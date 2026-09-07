import { describe, expect, it } from "vitest"

import { formatRelative } from "./format"

const now = Date.parse("2030-06-01T12:00:00Z")

describe("relative time", () => {
  it("stays on the relative scale at every magnitude", () => {
    expect(formatRelative("2030-06-01T11:59:59Z", now)).toBe("just now")
    expect(formatRelative("2030-06-01T11:59:00Z", now)).toBe("1m ago")
    expect(formatRelative("2030-06-01T11:00:00Z", now)).toBe("1h ago")
    expect(formatRelative("2030-05-30T12:00:00Z", now)).toBe("2d ago")
    expect(formatRelative("2030-04-01T12:00:00Z", now)).toBe("2mo ago")
    expect(formatRelative("2027-06-01T12:00:00Z", now)).toBe("3y ago")
  })

  it("never returns a form wider than the narrowest column can hold", () => {
    /*
     * The cell sits in an auto-layout table, so one wide result moves every
     * column beside it. Eight characters is what the relative forms fit in;
     * anything falling outside that budget shifts the table instead.
     */
    const spans = [-31_536_000, -3_600, -61, -1, 0, 30, 60, 3_600, 86_400, 2_592_000, 31_536_000, 315_360_000]
    for (const offset of spans) {
      const rendered = formatRelative(new Date(now - offset * 1000).toISOString(), now)
      expect(rendered.length, rendered).toBeLessThanOrEqual(8)
    }
  })

  it("reads a timestamp marginally ahead of the local clock as just now", () => {
    /*
     * The Controller stamps these, the browser renders them, and only one of the
     * two clocks is trusted. A machine that has drifted or just woken puts a
     * fresh timestamp a second or two in the future, which is skew rather than
     * information: it must not switch the cell to a full absolute date and drag
     * the column width -- and the whole table -- with it.
     */
    for (const value of ["2030-06-01T12:00:01Z", "2030-06-01T12:00:30Z", "2030-06-01T12:00:59Z"]) {
      expect(formatRelative(value, now)).toBe("just now")
    }
  })

  it("names the direction when the clocks disagree beyond plausible skew", () => {
    // Worth surfacing, but on the same scale: a wider kind of value would move the table.
    expect(formatRelative("2030-06-01T13:00:00Z", now)).toBe("in 1h")
    expect(formatRelative("2031-06-01T12:00:00Z", now)).toBe("in 1y")
  })

  it("returns a placeholder for absent and unparseable values", () => {
    expect(formatRelative(null, now)).toBe("--")
    expect(formatRelative("not-a-date", now)).toBe("--")
  })
})
