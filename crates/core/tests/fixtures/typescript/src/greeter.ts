// Builds greetings.
import { write } from "./io";

export interface Greeting {
  text: string;
}

export class Greeter {
  greet(name: string): Greeting {
    const text = `hi ${name}`;
    console.info("greeting", name);
    try {
      write(text);
    } catch (error) {
      console.error(error);
      throw error;
    } finally {
      expect(text).toBeDefined();
    }
    return { text };
  }
}

export const shout = (greeting: Greeting): string => greeting.text.toUpperCase();

export function load(url: string) {
  return fetch(url)
    .then((response) => response.json())
    .catch((error) => {
      logger.warn(error);
      return null;
    });
}
