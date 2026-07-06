/* Generated TypeScript surface for SafeMesh WASM bindings. */
export class SafeMeshGCounter {
  constructor(replicas: number);
  applyBump(replica: number, tally: bigint): void;
  value(): bigint;
  state(): BigUint64Array;
  free(): void;
}

export function gcounterDeltaToWire(replica: number, tally: bigint): Uint8Array;
