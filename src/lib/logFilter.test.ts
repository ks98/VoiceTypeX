// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { extractLevel, matchesFilter } from "./logFilter";

describe("extractLevel", () => {
  it("extracts each log level from a `[LEVEL] target - message` line", () => {
    expect(extractLevel("[TRACE] audio - capture started")).toBe("TRACE");
    expect(extractLevel("[DEBUG] stt - decoding chunk")).toBe("DEBUG");
    expect(extractLevel("[INFO ] pipeline - ready")).toBe("INFO");
    expect(extractLevel("[WARN ] injection - retrying")).toBe("WARN");
    expect(extractLevel("[ERROR] provider - request failed")).toBe("ERROR");
  });

  it("trims the padding the ring buffer adds to short levels", () => {
    expect(extractLevel("[INFO ] x - msg")).toBe("INFO");
    expect(extractLevel("[WARN ] x - msg")).toBe("WARN");
  });

  it("uppercases lowercase levels", () => {
    expect(extractLevel("[error] x - boom")).toBe("ERROR");
  });

  it("returns null for a line without a level prefix", () => {
    expect(extractLevel("just a plain message line")).toBeNull();
  });

  it("returns null when the line does not start with `[`", () => {
    expect(extractLevel("INFO] target - message")).toBeNull();
  });

  it("returns null when there is no closing bracket", () => {
    expect(extractLevel("[INFO target - message")).toBeNull();
  });

  it("returns null for lines shorter than 7 chars", () => {
    expect(extractLevel("[INFO]")).toBeNull();
    expect(extractLevel("")).toBeNull();
  });
});

describe("matchesFilter", () => {
  it("passes every line through the 'all' filter", () => {
    expect(matchesFilter("[INFO ] x - msg", "all")).toBe(true);
    expect(matchesFilter("[ERROR] x - boom", "all")).toBe(true);
    expect(matchesFilter("no level here", "all")).toBe(true);
  });

  it("'error' matches only ERROR lines", () => {
    expect(matchesFilter("[ERROR] x - boom", "error")).toBe(true);
    expect(matchesFilter("[WARN ] x - careful", "error")).toBe(false);
    expect(matchesFilter("[INFO ] x - msg", "error")).toBe(false);
  });

  it("'warn' matches WARN and ERROR lines", () => {
    expect(matchesFilter("[WARN ] x - careful", "warn")).toBe(true);
    expect(matchesFilter("[ERROR] x - boom", "warn")).toBe(true);
    expect(matchesFilter("[INFO ] x - msg", "warn")).toBe(false);
    expect(matchesFilter("[DEBUG] x - noise", "warn")).toBe(false);
  });

  it("excludes lines without a parseable level from a specific filter", () => {
    expect(matchesFilter("no level here", "error")).toBe(false);
    expect(matchesFilter("no level here", "warn")).toBe(false);
  });
});
