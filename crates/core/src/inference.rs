//! Calls to models: one OpenAI-compatible client for embeddings, and `OpenRouter`'s Decisions API
//! for the typed yes/no questions the analyses ask.
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

/// Version of the decision question and its criteria, which are the model's whole prompt. Shorter
/// wording measured clearly worse (it separated copies from look-alikes far less), so a change
/// must be measured again, and it changes this, so cached answers to the old one are not reused.
pub(crate) const PROMPT_VERSION: &str = "same-behaviour-2";

/// Characters of each function put in a question: enough for the body of nearly every function,
/// bounded so that one huge function cannot dominate the cost of a call.
const QUESTION_CHARS_MAX: usize = 4000;

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("cannot reach the model API: {0}")]
    Transport(String),
    #[error("the model API answered {status}: {body}")]
    Status { status: u16, body: String },
    #[error("unexpected response from the model API: {0}")]
    Response(String),
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

    /// Posts `body` to `path` under the base URL.
    fn post(&self, path: &str, body: &Value) -> Result<Value, InferenceError> {
        self.post_to(&format!("{}{path}", self.base_url), body)
    }

    /// Posts `body` to `path` under the API's root, which is the base URL without its version:
    /// the Decisions API is served beside `/v1`, not under it.
    fn post_root(&self, path: &str, body: &Value) -> Result<Value, InferenceError> {
        let root = self.base_url.strip_suffix("/v1").unwrap_or(&self.base_url);
        self.post_to(&format!("{root}{path}"), body)
    }

    /// Posts `body` to `url` and returns the JSON answer, retrying rate limits, server errors
    /// and transport failures with doubling waits.
    fn post_to(&self, url: &str, body: &Value) -> Result<Value, InferenceError> {
        let payload = body.to_string();
        let mut wait = Duration::from_millis(BACKOFF_MS);
        let mut attempt = 1;
        loop {
            let outcome = self.send(url, &payload);
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

/// The decision models whose bar was measured, by id prefix (the router's alias marker aside), and
/// the probability of "same thing" at which each one's answer is accepted. Probabilities do not
/// transfer between models, so a model is used only once it has a row. Measured on 289
/// hand-labelled candidate pairs (52 copies), the bar with the best F1:
/// - Jev: 0.65 (precision 0.54, recall 0.71, F1 0.61; 0.63 and 0.70 give 0.60 and 0.59). It
///   ranks copies above look-alikes well (ROC-AUC 0.88) but is under-confident on true copies.
/// - `OpenAI`'s gpt-6-luna decisions: 0.70 (precision 0.51, recall 0.67, F1 0.58; 0.80 gives 0.57).
///   Its AUC is 0.89 and it is more confident, so the F1 curve is flat from 0.5 to 0.8.
const PROBABILITY_MIN: [(&str, f64); 2] = [
    ("typesafe/jev-", 0.65),
    ("openai/gpt-6-luna-decisions", 0.70),
];

/// The acceptance bar measured for `model`, `None` for a model nobody has measured.
pub(crate) fn probability_min(model: &str) -> Option<f64> {
    let id = model.trim_start_matches('~');
    PROBABILITY_MIN
        .iter()
        .find(|(prefix, _)| id.starts_with(prefix))
        .map(|&(_, bar)| bar)
}

/// What the question asks, and when it is true or false. `{other}` is the candidate's name.
const INSTRUCTIONS: &str = "Do function_a and {other} serve the same purpose, such that \
    they could be replaced by a single shared function (perhaps parameterised or generic)?";
const TRUE_MEANS: &str = "Both compute the same result and have the same effects for the same \
    inputs, even if written in different languages or styles, or against different backends of \
    one interface; one could replace the other or both could be one function.";
const FALSE_MEANS: &str = "They do different things: different inputs or outputs, different \
    effects, different steps of a process, inverse operations, or merely similar-looking \
    boilerplate, wrappers or tests that check different behaviour.";

/// A decision model behind `OpenRouter`'s Decisions API: one yes/no ("noul") question per
/// candidate, all about the same function, so they share one request and its state. The answer
/// is the probability of yes, and output tokens are free.
pub(crate) struct DecisionModel<'a> {
    pub client: &'a Client,
    pub model: &'a str,
}

/// The part of a function that goes in a question.
fn cut(text: &str) -> String {
    text.chars().take(QUESTION_CHARS_MAX).collect()
}

fn request(model: &str, first: &str, others: &[&str]) -> Value {
    let mut state = serde_json::Map::new();
    let mut questions = serde_json::Map::new();
    state.insert("function_a".to_owned(), Value::from(cut(first)));
    for (position, other) in others.iter().enumerate() {
        let name = format!("function_b{}", position + 1);
        questions.insert(
            format!("same_b{}", position + 1),
            json!({
                "type": "noul",
                "instructions": INSTRUCTIONS.replace("{other}", &name),
                "criteria": { "true": TRUE_MEANS, "false": FALSE_MEANS },
            }),
        );
        state.insert(name, Value::from(cut(other)));
    }
    json!({ "model": model, "state": state, "questions": questions })
}

fn probabilities(response: &Value, count: usize) -> Result<Vec<f64>, InferenceError> {
    (1..=count)
        .map(|position| {
            response
                .pointer(&format!("/answers/same_b{position}/noul"))
                .and_then(Value::as_f64)
                .map(|probability| probability.clamp(0.0, 1.0))
                .ok_or_else(|| InferenceError::Response(format!("no answer {position}")))
        })
        .collect()
}

impl DecisionModel<'_> {
    /// For each of `others`, the probability that it and `first` do the same thing, in order,
    /// from one call.
    pub fn decide(&self, first: &str, others: &[&str]) -> Result<Vec<f64>, InferenceError> {
        if others.is_empty() {
            return Ok(Vec::new());
        }
        let response = self
            .client
            .post_root("/alpha/decisions", &request(self.model, first, others))?;
        probabilities(&response, others.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_per_candidate_about_the_same_function() {
        let long = "x".repeat(QUESTION_CHARS_MAX + 100);
        let body = request("typesafe/jev-1.14", &long, &["one", "two", "three"]);
        let state = body["state"].as_object().unwrap();
        let questions = body["questions"].as_object().unwrap();
        assert_eq!(state.len(), 4);
        assert_eq!(questions.len(), 3);
        assert_eq!(
            state["function_a"].as_str().unwrap().len(),
            QUESTION_CHARS_MAX
        );
        // Each question names its own candidate and no other.
        for position in 1..=3 {
            let question = &questions[&format!("same_b{position}")];
            assert_eq!(question["type"], "noul");
            assert!(state.contains_key(&format!("function_b{position}")));
            let instructions = question["instructions"].as_str().unwrap();
            assert!(instructions.contains(&format!("function_b{position} ")));
            assert!(!instructions.contains(&format!("function_b{} ", position % 3 + 1)));
        }
    }

    #[test]
    fn answers_are_read_in_the_order_asked() {
        let response = json!({ "answers": {
            "same_b1": { "type": "noul", "noul": 0.9 },
            "same_b2": { "type": "noul", "noul": 0.1 },
        }});
        assert_eq!(probabilities(&response, 2).unwrap(), vec![0.9, 0.1]);
        assert!(matches!(
            probabilities(&response, 3),
            Err(InferenceError::Response(_))
        ));
    }

    #[test]
    fn only_measured_models_have_a_bar_and_an_alias_shares_its_models() {
        assert!(probability_min("typesafe/jev-1.14").is_some());
        assert_eq!(
            probability_min("~typesafe/jev-latest"),
            probability_min("typesafe/jev-1.14")
        );
        assert!(probability_min("qwen/qwen3-next-80b-a3b-instruct").is_none());
    }

    #[test]
    fn vectors_are_scaled_to_unit_length() {
        let vector = unit(vec![3.0, 4.0]);
        assert!((vector.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-6);
        assert_eq!(unit(vec![0.0, 0.0]), vec![0.0, 0.0]);
    }
}
