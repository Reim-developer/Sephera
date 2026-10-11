/**
 * The resizer's arithmetic.
 *
 * The drag is a function of where it began and where the pointer is, and that is
 * the whole reason it is exported: jsdom has no layout, so a drag simulated through
 * a synthetic pointer event measures nothing. Driving the arithmetic directly is
 * the only way to test what the handle does.
 *
 * The direction is the thing that broke. The panel is anchored to the window's
 * right edge, so dragging left *widens* it -- the opposite sign to the pointer's
 * movement -- and the previous version measured from the handle's own edge and
 * added the current width, which is not a width at all.
 */

import { describe, expect, it } from "vitest";

import { resizedWidth } from "@/components/PanelResizer";

describe("resizedWidth", () => {
  it("grows the panel when the pointer moves left", () => {
    // Anchored to the right edge, so leftwards is wider.
    expect(resizedWidth(320, 500, 450)).toBe(370);
  });

  it("narrows the panel when the pointer moves right", () => {
    expect(resizedWidth(320, 500, 560)).toBe(260);
  });

  it("stays put when the pointer has not moved", () => {
    expect(resizedWidth(320, 500, 500)).toBe(320);
  });

  it("is clamped at both ends, so the handle cannot leave the range", () => {
    // Far left: the panel grows towards its ceiling, because leftwards *widens* a
    // right-anchored panel. A short declaration row is unreadable much below the
    // floor; the ceiling is where the panel becomes the content and the table a
    // sliver.
    expect(resizedWidth(320, 500, -10000)).toBe(760);
    expect(resizedWidth(320, 500, 10000)).toBe(220);
  });

  it("clamps into range from either side of the limits", () => {
    // Starting outside the range is possible if the window was resized; a drag
    // must pull it back in rather than leaving it stuck.
    expect(resizedWidth(900, 500, 500)).toBe(760);
    expect(resizedWidth(50, 500, 500)).toBe(220);
  });

  it("rounds to whole pixels", () => {
    // A fractional width is a width the CSS will render at, so rounding here
    // keeps the handle and the panel in step.
    expect(resizedWidth(320, 500.7, 450.2)).toBe(371);
  });
});
