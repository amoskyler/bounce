/**
 * The attachment shape, and the one place it is translated.
 *
 * Three declarations of the same thing exist by necessity — the renderer's, the
 * napi struct's, and this one in between — and the translation between them is
 * written out field by field. That is what makes it worth isolating and
 * testing: a field added to the other two and forgotten here is dropped in
 * silence. `path` was, and a large attachment reached the engine with neither
 * its contents nor anywhere to read them from, which it could only report as
 * an empty file.
 */

import { existsSync } from 'node:fs';

/** What the native module takes. Every field is required on that side. */
export interface NativeAttachment {
  name: string;
  data: Buffer;
  isImage: boolean;
  width: number;
  height: number;
  blurHash: string;
  path: string;
}

/** The same, as it arrives over IPC from the renderer. */
export interface OutgoingAttachment {
  name: string;
  data: Uint8Array;
  isImage: boolean;
  width: number;
  height: number;
  blurHash?: string;
  /**
   * Where the file is on disk, for one too large to hold in memory.
   *
   * Set instead of `data`, never as well as it. The engine streams from the
   * path when there is one and embeds the bytes when there is not.
   */
  path?: string;
}

/**
 * Adapt a renderer attachment for the native module.
 *
 * Structured cloning delivers the bytes as a `Uint8Array`, and napi-rs wants a
 * `Buffer`. `Buffer.from(view)` would copy; the three-argument form wraps the
 * same memory, which for a 20 MiB attachment is worth the care.
 */
export function toNative(attachment: OutgoingAttachment): NativeAttachment {
  const bytes = attachment.data;
  return {
    name: attachment.name,
    data: Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength),
    isImage: attachment.isImage,
    width: attachment.width,
    height: attachment.height,
    blurHash: attachment.blurHash ?? '',
    path: attachment.path ?? '',
  };
}

/**
 * Refuse an attachment the engine could not possibly send, and say why.
 *
 * This is the last point at which the shape is still describable. Past it the
 * engine sees an attachment with no bytes and can only say so — true, and no
 * help in working out which of the layers between a file picker and a chunk
 * hash dropped what.
 *
 * A path is checked against the filesystem rather than trusted, because one
 * that does not resolve fails much later, inside a hash loop, as an IO error
 * with no mention of the attachment it came from.
 */
export function checkAttachments(
  attachments: readonly OutgoingAttachment[],
  exists: (path: string) => boolean = existsSync,
): void {
  for (const attachment of attachments) {
    const name = attachment.name || 'an attachment';
    const hasBytes = (attachment.data?.length ?? 0) > 0;
    const path = attachment.path ?? '';

    if (!hasBytes && !path) {
      throw new Error(
        `${name} arrived with no contents and no path on disk. That is a bug in ` +
          `this client rather than anything you did.`,
      );
    }

    if (!hasBytes && !exists(path)) {
      throw new Error(`${name} could not be read from ${path}. Has it moved?`);
    }
  }
}
