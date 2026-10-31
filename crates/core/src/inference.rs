//! Calls to models: one OpenAI-compatible client for embeddings and chat completions, and the
//! [`DecisionModel`] trait the analyses ask typed questions through.
//!
//! Nothing here decides what to ask or caches an answer; analyses do, so a call is made only for
//! what the index lacks.

use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use ureq::Agent;

/// How long one request may take in all. Embedding a batch of long functions is the slowest call
/// and takes a few seconds; a minute means the provider is stuck.
const REQUEST_TIMEOUT_S: u64 = 60;

/// Attempts per request. Rate limits and provider hiccups clear within a few seconds, so with
/// [`BACKOFF_MS`] doubling the last try comes about 15 seconds after the first.
const ATTEMPTS_MAX: u32 = 5;

/// Wait before the first retry; each further retry waits twice as long.
const BACKOFF_MS: u64 = 1000;

/// Largest response read. A batch of [`crate::semantic`]'s embeddings is a few megabytes of JSON.
const RESPONSE_BYTES_MAX: u64 = 64 * 1024 * 1024;

/// Components kept of every embedding. The embedding models worth using are trained so that a
/// prefix of the vector is itself an embedding (Matryoshka); 1024 of qwen3-embedding's 4096 keep
/// the margin between a cross-language copy and unrelated code while cutting storage and the
/// nearest-neighbour search to a quarter.
pub(crate) const EMBEDDING_DIMENSIONS: usize = 1024;

/// Version of [`SameBehaviour`]'s prompt; a change to the wording changes it, so cached answers
/// to the old one are not reused.
pub(crate) const SAME_BEHAVIOUR_PROMPT_VERSION: &str = "same-behaviour-1";

/// Characters of each function put in a question: enough for the body of nearly every function,
/// bounded so that one huge function cannot dominate the cost of a call.
const QUESTION_CHARS_MAX: usize = 4000;

/// Tokens a decision may generate: the answer is one word.
const ANSWER_TOKENS_MAX: u32 = 4;

/// How many candidate tokens the provider returns probabilities for at the answer position.
const LOGPROBS_TOP: u32 = 5;

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("cannot reach the model API: {0}")]
    Transport(String),
    #[error("the model API answered {status}: {body}")]
    Status { status: u16, body: String },
    #[error("unexpected response from the model API: {0}")]
    Response(String),
    #[error("the model gave no yes or no: {0:?}")]
    Undecided(String),
}

/// An OpenAI-compatible API, and what its calls have cost so far.
pub(crate) struct Client {
    agent: Agent,
    base_url: String,
    key: String,
    /// Spend the provider reported, in millionths of a dollar.
    cost_micro_usd: AtomicU64,
    /// Answers received, and how many of them reported a cost.
    answers: AtomicU64,
    costed_answers: AtomicU64,
}

impl Client {
    pub fn new(base_url: &str, key: String) -> Self {
        let agent = Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(REQUEST_TIMEOUT_S)))
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            agent,
            base_url: base_url.trim_end_matches('/').to_owned(),
            key,
            cost_micro_usd: AtomicU64::new(0),
            answers: AtomicU64::new(0),
            costed_answers: AtomicU64::new(0),
        }
    }

    /// Dollars the provider has charged for this client's calls, in millionths; `None` when calls
    /// were made and the provider did not say what they cost.
    pub fn cost_micro_usd(&self) -> Option<u64> {
        let costed = self.costed_answers.load(Ordering::Relaxed) > 0;
        (costed || self.answers.load(Ordering::Relaxed) == 0)
            .then(|| self.cost_micro_usd.load(Ordering::Relaxed))
    }

    /// Posts `body` and returns the JSON answer, retrying rate limits, server errors and
    /// transport failures with doubling waits.
    fn post(&self, path: &str, body: &Value) -> Result<Value, InferenceError> {
        let url = format!("{}{path}", self.base_url);
        let payload = body.to_string();
        let mut wait = Duration::from_millis(BACKOFF_MS);
        let mut attempt = 1;
        loop {
            let outcome = self.send(&url, &payload);
            let retryable = match &outcome {
                Err(InferenceError::Transport(_)) => true,
                Err(InferenceError::Status { status, .. }) => *status == 429 || *status >= 500,
                _ => false,
            };
            if !retryable || attempt == ATTEMPTS_MAX {
                return outcome;
            }
            thread::sleep(wait);
            wait *= 2;
            attempt += 1;
        }
    }

    fn send(&self, url: &str, payload: &str) -> Result<Value, InferenceError> {
        let mut response = self
            .agent
            .post(url)
            .header("Authorization", format!("Bearer {}", self.key))
            .header("Content-Type", "application/json")
            .send(payload)
            .map_err(|error| InferenceError::Transport(error.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .with_config()
            .limit(RESPONSE_BYTES_MAX)
            .read_to_string()
            .map_err(|error| InferenceError::Transport(error.to_string()))?;
        if status != 200 {
            return Err(InferenceError::Status {
                status,
                body: text.chars().take(300).collect(),
            });
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|error| InferenceError::Response(error.to_string()))?;
        self.answers.fetch_add(1, Ordering::Relaxed);
        // OpenRouter reports what a call cost; other providers leave it out.
        if let Some(cost) = value.pointer("/usage/cost").and_then(Value::as_f64) {
            self.costed_answers.fetch_add(1, Ordering::Relaxed);
            // Rounded to a whole number of millionths, and never negative.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a rounded, non-negative cost far below u64::MAX"
            )]
            let micro = (cost.max(0.0) * 1e6).round() as u64;
            self.cost_micro_usd.fetch_add(micro, Ordering::Relaxed);
        }
        Ok(value)
    }

    /// Unit-length vectors for `texts`, in order, cut to [`EMBEDDING_DIMENSIONS`].
    #[expect(
        clippy::cast_possible_truncation,
        reason = "embedding components are far inside f32 range"
    )]
    pub fn embed(&self, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, InferenceError> {
        let response = self.post("/embeddings", &json!({ "model": model, "input": texts }))?;
        let data = response
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| InferenceError::Response("no data".to_owned()))?;
        let mut vectors: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        for item in data {
            let position = item
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|index| usize::try_from(index).ok())
                .filter(|&index| index < texts.len())
                .ok_or_else(|| InferenceError::Response("bad index".to_owned()))?;
            let vector: Vec<f32> = item
                .get("embedding")
                .and_then(Value::as_array)
                .ok_or_else(|| InferenceError::Response("no embedding".to_owned()))?
                .iter()
                .take(EMBEDDING_DIMENSIONS)
                .map(|component| component.as_f64().unwrap_or(0.0) as f32)
                .collect();
            vectors[position] = Some(unit(vector));
        }
        vectors
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| InferenceError::Response("fewer embeddings than inputs".to_owned()))
    }
}

/// The vector scaled to length 1, so that a dot product is a cosine.
pub(crate) fn unit(mut vector: Vec<f32>) -> Vec<f32> {
    let length = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if length > 0.0 {
        for component in &mut vector {
            *component /= length;
        }
    }
    vector
}

/// "Do these two functions do the same thing?"
pub(crate) struct SameBehaviour<'a> {
    pub first: &'a str,
    pub second: &'a str,
}

/// Answers typed yes/no questions with a probability and no prose.
pub(crate) trait DecisionModel: Sync {
    /// The model's name, which keys its cached answers.
    fn name(&self) -> &str;

    /// The probability that the two functions do the same thing.
    fn decide(&self, question: &SameBehaviour<'_>) -> Result<f64, InferenceError>;
}

/// A [`DecisionModel`] over chat completions: the question goes in with a one-word answer
/// required, and the probability is read from the answer token's log-probabilities. A provider
/// that returns none leaves the answer itself, which is 1 or 0.
pub(crate) struct ChatDecisionModel<'a> {
    pub client: &'a Client,
    pub model: &'a str,
}

fn prompt(question: &SameBehaviour<'_>) -> String {
    let cut = |text: &str| text.chars().take(QUESTION_CHARS_MAX).collect::<String>();
    format!(
        "Do these two functions do the same thing? Ignore names, language and style; compare what \
         they compute and which effects they have.\n\nFunction 1:\n{}\n\nFunction 2:\n{}\n\n\
         Same thing?",
        cut(question.first),
        cut(question.second)
    )
}

impl DecisionModel for ChatDecisionModel<'_> {
    fn name(&self) -> &str {
        self.model
    }

    fn decide(&self, question: &SameBehaviour<'_>) -> Result<f64, InferenceError> {
        let response = self.client.post(
            "/chat/completions",
            &json!({
                "model": self.model,
                "messages": [
                    { "role": "system", "content": "Answer with exactly one word: Yes or No." },
                    { "role": "user", "content": prompt(question) },
                ],
                "max_tokens": ANSWER_TOKENS_MAX,
                "temperature": 0,
                "logprobs": true,
                "top_logprobs": LOGPROBS_TOP,
            }),
        )?;
        let choice = response
            .pointer("/choices/0")
            .ok_or_else(|| InferenceError::Response("no choices".to_owned()))?;
        let text = choice
            .pointer("/message/content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        // The answer is the first token with a word in it; a leading newline or space is not.
        if let Some(tokens) = choice
            .pointer("/logprobs/content")
            .and_then(Value::as_array)
            && let Some(candidates) = answer_candidates(tokens)
            && let Some(probability) = yes_probability(candidates)
        {
            return Ok(probability);
        }
        match word(text).as_str() {
            "yes" => Ok(1.0),
            "no" => Ok(0.0),
            _ => Err(InferenceError::Undecided(text.chars().take(40).collect())),
        }
    }
}

/// The alternatives the provider weighed at the answer token: the first token with a word in it,
/// because a leading newline or space is not the decision.
fn answer_candidates(tokens: &[Value]) -> Option<&[Value]> {
    let token_word = |token: &Value| {
        word(
            token
                .get("token")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
    };
    let token = tokens.iter().find(|token| !token_word(token).is_empty())?;
    // Only a yes or no is a decision; anything else is not read as one.
    matches!(token_word(token).as_str(), "yes" | "no").then_some(())?;
    token
        .get("top_logprobs")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
}

/// An answer's first word, lowercase and without punctuation.
fn word(text: &str) -> String {
    text.trim()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_lowercase()
}

/// Yes over yes plus no among the candidate tokens, `None` when it offers neither.
fn yes_probability(candidates: &[Value]) -> Option<f64> {
    let (mut yes, mut no) = (0.0, 0.0);
    for candidate in candidates {
        let token = candidate
            .get("token")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let logprob = candidate.get("logprob").and_then(Value::as_f64)?;
        match word(token).as_str() {
            "yes" => yes += logprob.exp(),
            "no" => no += logprob.exp(),
            _ => {}
        }
    }
    (yes + no > 0.0).then(|| yes / (yes + no))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates(tokens: &[(&str, f64)]) -> Vec<Value> {
        tokens
            .iter()
            .map(|(token, p)| json!({ "token": token, "logprob": f64::ln(*p) }))
            .collect()
    }

    #[test]
    fn probability_pools_spellings_of_each_answer() {
        let found = yes_probability(&candidates(&[
            ("Yes", 0.6),
            (" yes", 0.1),
            ("No", 0.2),
            ("Same", 0.1),
        ]));
        assert!((found.unwrap() - 0.7 / 0.9).abs() < 1e-9);
    }

    #[test]
    fn no_yes_or_no_among_the_candidates_is_no_probability() {
        assert_eq!(yes_probability(&candidates(&[("Same", 0.9)])), None);
    }

    #[test]
    fn answers_are_read_without_punctuation_or_case() {
        assert_eq!(word("No."), "no");
        assert_eq!(word("  YES\n"), "yes");
    }

    #[test]
    fn the_answer_token_is_the_first_one_with_a_word_in_it() {
        let tokens: Vec<Value> = [("\n", 1.0), ("No", 0.9)]
            .iter()
            .map(|(token, p)| {
                json!({
                    "token": token,
                    "top_logprobs": [{ "token": token, "logprob": f64::ln(*p) }, { "token": "Yes", "logprob": f64::ln(0.1) }],
                })
            })
            .collect();
        let answer = answer_candidates(&tokens).unwrap();
        assert!((yes_probability(answer).unwrap() - 0.1 / 1.0).abs() < 1e-9);
    }

    #[test]
    fn an_answer_that_is_neither_yes_nor_no_is_not_a_decision() {
        let tokens = vec![json!({
            "token": "Different",
            "top_logprobs": [{ "token": "Different", "logprob": f64::ln(0.99) }, { "token": "Yes", "logprob": f64::ln(0.01) }],
        })];
        assert_eq!(answer_candidates(&tokens), None);
    }

    #[test]
    fn vectors_are_scaled_to_unit_length() {
        let vector = unit(vec![3.0, 4.0]);
        assert!((vector.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-6);
        assert_eq!(unit(vec![0.0, 0.0]), vec![0.0, 0.0]);
    }
}
