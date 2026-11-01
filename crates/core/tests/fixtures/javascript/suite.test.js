import { describe, it, test } from "vitest";

describe("box", () => {
  it("opens", () => {
    expect(open()).toBe(1);
  });

  it.only("closes", async function () {
    expect(close()).toBe(0);
  });

  describe.each([1, 2])("sizes %i", (size) => {
    test.skip("fits", () => {
      expect(size).toBeGreaterThan(0);
    });
  });
});

test.each([[1], [2]])("doubles %i", (n) => {
  expect(n * 2).toBeGreaterThan(n);
});

it("is not a test without a callback", helper);

test.describe("flows", () => {
  test("doesn't break", () => {});
});
