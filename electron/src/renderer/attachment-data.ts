/**
 * Turning a finished download into something an `<img>` can point at.
 *
 * The bytes live in the main process. A bubble asks for them by file id once
 * the engine says the file is complete, and gets back an object URL it can use
 * for the rest of the session.
 *
 * ## Why the cache is module-level
 *
 * A `blob:` URL is a handle into the renderer's memory, and creating a second
 * one for the same file allocates a second copy. The same image can be on
 * screen in several places at once — the bubble, the full-size viewer — and a
 * conversation can be closed and reopened, so the URL is keyed by file id and
 * kept, rather than being tied to a component's lifetime.
 *
 * The cache is bounded: an attachment is at most 20 MiB, so a few dozen of them
 * is a real amount of memory to be holding for pictures nobody is looking at.
 * Past the limit, the least recently used URL is revoked.
 */

/** How many decoded attachments to keep URLs for. */
const MAX_CACHED_FILES = 32;

/** File id to object URL, in least-recently-used order. */
const urls = new Map<string, string>();

/** Files being fetched, so two bubbles do not ask for the same one twice. */
const inFlight = new Map<string, Promise<string | null>>();

/** The cached URL for a file, if it has already been fetched. */
export function cachedFileUrl(fileId: string): string | undefined {
  const url = urls.get(fileId);
  if (url === undefined) return undefined;

  // Re-insert, so this is now the most recently used.
  urls.delete(fileId);
  urls.set(fileId, url);
  return url;
}

/**
 * Fetch a file's bytes and return a URL for them.
 *
 * Resolves to null while the file is still downloading — the engine returns
 * nothing until every chunk is present, because a half-assembled image is worse
 * than none.
 */
export function fileUrl(fileId: string, mimeType?: string): Promise<string | null> {
  const cached = cachedFileUrl(fileId);
  if (cached !== undefined) return Promise.resolve(cached);

  const existing = inFlight.get(fileId);
  if (existing) return existing;

  const request = window.bounce
    .fileData(fileId)
    .then((bytes) => {
      if (!bytes) return null;

      // `Uint8Array<ArrayBufferLike>` is not a `BlobPart`, because the buffer
      // could in principle be shared; copying into a plain one settles it and
      // costs nothing an attachment this size would notice.
      const copy = new Uint8Array(bytes.length);
      copy.set(bytes);

      const blob = new Blob([copy], mimeType ? { type: mimeType } : undefined);
      const url = URL.createObjectURL(blob);
      remember(fileId, url);
      return url;
    })
    .catch(() => null)
    .finally(() => {
      inFlight.delete(fileId);
    });

  inFlight.set(fileId, request);
  return request;
}

function remember(fileId: string, url: string): void {
  urls.set(fileId, url);

  while (urls.size > MAX_CACHED_FILES) {
    // Map iteration is insertion-ordered, and every hit re-inserts, so the
    // first key is the least recently used.
    const oldest = urls.keys().next();
    if (oldest.done) break;
    const stale = urls.get(oldest.value);
    if (stale) URL.revokeObjectURL(stale);
    urls.delete(oldest.value);
  }
}

/** Drop every cached URL. Used when the window is going away. */
export function clearFileUrls(): void {
  for (const url of urls.values()) URL.revokeObjectURL(url);
  urls.clear();
}
