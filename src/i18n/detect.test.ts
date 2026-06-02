// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { pickSupported } from "./detect";

describe("pickSupported", () => {
  it("strips region suffixes (BCP-47 hyphen and underscore)", () => {
    expect(pickSupported("de-DE")).toBe("de");
    expect(pickSupported("de_DE")).toBe("de");
    expect(pickSupported("en-US")).toBe("en");
    expect(pickSupported("it_IT")).toBe("it");
  });

  it("accepts bare supported codes", () => {
    expect(pickSupported("de")).toBe("de");
    expect(pickSupported("fr")).toBe("fr");
    expect(pickSupported("es")).toBe("es");
    expect(pickSupported("en")).toBe("en");
  });

  it("is case-insensitive", () => {
    expect(pickSupported("DE")).toBe("de");
    expect(pickSupported("De-de")).toBe("de");
  });

  it("falls back to en for unsupported languages", () => {
    expect(pickSupported("pt-BR")).toBe("en");
    expect(pickSupported("zh")).toBe("en");
    expect(pickSupported("nl")).toBe("en");
  });

  it("falls back to en for null/undefined/empty", () => {
    expect(pickSupported(null)).toBe("en");
    expect(pickSupported(undefined)).toBe("en");
    expect(pickSupported("")).toBe("en");
  });
});
