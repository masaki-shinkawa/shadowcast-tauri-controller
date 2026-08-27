import { describe, expect, it } from "vitest";
import { framePayloadToBytes } from "./tauri";

describe("framePayloadToBytes", () => {
  it("keeps the ArrayBuffer backing a Uint8Array payload", () => {
    const payload = new Uint8Array([0xff, 0xd8, 0xff]);
    expect(framePayloadToBytes(payload).buffer).toBe(payload.buffer);
  });

  it("normalizes array and ArrayBuffer payloads", () => {
    expect([...framePayloadToBytes([1, 2, 3])]).toEqual([1, 2, 3]);
    expect([...framePayloadToBytes(new Uint8Array([4, 5]).buffer)]).toEqual([4, 5]);
  });
});
