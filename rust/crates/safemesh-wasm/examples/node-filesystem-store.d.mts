// Type declarations for the existing Node example Store.
export class FileSystemStore {
  constructor(root: string, anchorPath: string);
  open(mode: "fresh" | "restart", writer: bigint): unknown;
  readCommitted(lease: unknown): { bytes: Uint8Array; revision: bigint; anchor: bigint };
  commit(lease: unknown, expectedRevision: bigint, nextBytes: Uint8Array): bigint;
  close(lease: unknown): void;
}
