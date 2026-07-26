/**
 * Preparing an image for the engine.
 *
 * The engine never decodes an image. It stores bytes, splits them into chunks,
 * and repeats whatever dimensions and BlurHash it was handed — so working those
 * out is the client's job, and getting them wrong shows up on somebody else's
 * screen as a picture that reserves the wrong space or blurs into the wrong
 * colours.
 *
 * Three places need this: the composer, the profile picture, and a group
 * picture. It lives here rather than being written three times, because the
 * BlurHash parameters have to match the Go client exactly and a copy that
 * drifts is a copy that silently disagrees with it.
 */

import { blurHashFromImage } from './blurhash';
import type { OutgoingAttachment } from '../preload';

/** The largest file the engine will accept, mirroring `EMBEDDED_FILE_LIMIT`. */
export const IMAGE_SIZE_LIMIT = 20 * 1024 * 1024;

/** Formats worth offering in a picker. SVG is a document, not a bitmap. */
export const PICTURE_ACCEPT = 'image/png,image/jpeg,image/gif,image/webp';

/**
 * Read a file into the shape the bridge takes, decoding it to learn its size.
 *
 * Rejects anything that will not decode, because a "picture" the recipient
 * cannot render is worse than a refusal here: it would occupy a slot in the
 * profile and show as a broken frame on every device that fetched it.
 */
export async function prepareImage(file: File): Promise<OutgoingAttachment> {
  if (file.size > IMAGE_SIZE_LIMIT) {
    throw new Error(
      `That picture is ${Math.round(file.size / 1024 / 1024)} MB. The limit is ${
        IMAGE_SIZE_LIMIT / 1024 / 1024
      } MB.`,
    );
  }

  const bytes = new Uint8Array(await file.arrayBuffer());
  const mimeType = file.type || 'image/png';

  // Decoded from our own copy rather than from the File, so the result cannot
  // change under us if the original is moved or replaced mid-read.
  const url = URL.createObjectURL(new Blob([bytes], { type: mimeType }));

  try {
    const image = await new Promise<HTMLImageElement>((resolve, reject) => {
      const probe = new Image();
      probe.onload = () => resolve(probe);
      probe.onerror = () => reject(new Error('that file is not an image this build can read'));
      probe.src = url;
    });

    return {
      name: file.name || 'picture.png',
      data: bytes,
      isImage: true,
      width: image.naturalWidth,
      height: image.naturalHeight,
      // A failure here costs the placeholder, not the picture.
      blurHash: blurHashFromImage(image) ?? '',
    };
  } finally {
    URL.revokeObjectURL(url);
  }
}

/**
 * Open the system picture picker and hand back what was chosen.
 *
 * Resolves to null when the dialog is dismissed. The input is created and
 * discarded per call rather than kept in the tree: a hidden `<input>` that
 * outlives the dialog it belongs to is a reliable way to fire a stale handler.
 */
export function choosePicture(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = PICTURE_ACCEPT;

    input.addEventListener('change', () => {
      resolve(input.files?.[0] ?? null);
      input.remove();
    });

    // Firing on window focus catches a cancelled dialog, which emits no event
    // of its own. Deferred, because focus returns before `change` does.
    window.addEventListener(
      'focus',
      () => {
        window.setTimeout(() => {
          if (input.isConnected) {
            resolve(null);
            input.remove();
          }
        }, 400);
      },
      { once: true },
    );

    input.style.display = 'none';
    document.body.appendChild(input);
    input.click();
  });
}
