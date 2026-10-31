import { Greeter } from "./greeter";

it("greets", () => {
  expect(new Greeter().greet("a").text).toBe("hi a");
});
