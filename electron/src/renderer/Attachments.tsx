/**
 * Attachments: getting them into the composer, and showing them once sent.
 *
 * Four pieces, in the order a file passes through them: `useAttachmentIntake`
 * turns a paste, a drop, or a file picker into bytes held in memory;
 * `AttachmentTray` shows what is staged under the composer; `AttachmentList`
 * renders what arrived inside a bubble; and `ImageViewer` opens a picture full
 * screen.
 *
 * Nothing here touches the engine. The hook hands the composer plain bytes and
 * the composer decides what to do with them, which keeps the intake logic
 * testable and keeps this file free of any assumption about how a message is
 * sent.
 *
 * Object URLs are the one piece of state that outlives a render, so their
 * lifetime is managed explicitly: every URL this file creates is revoked when
 * its attachment is removed, when the tray is cleared after a send, or when the
 * composer unmounts. A chat window stays open for days, and a leaked object URL
 * pins its blob in memory for exactly that long.
 */

import * as React from 'react';

import { blurHashToDataUrl } from './blurhash';

import { CloseIcon } from './icons';
import { fileSize } from './format';
import type { Attachment } from '../preload';

/**
 * The largest file that may ride along inside a message.
 *
 * Mirrors `EMBEDDED_FILE_LIMIT` in `rust/libbounce/src/lib.rs`. This is not a
 * cap on what may be sent — anything larger is streamed from disk instead of
 * being embedded — it is the point at which reading the whole thing into
 * memory stops being reasonable.
 */
export const EMBEDDED_FILE_LIMIT = 20 * 1024 * 1024;

/**
 * What the "Photos & Videos" menu item narrows the picker to.
 *
 * Wildcards rather than a list of types, because the point is to hide the rest
 * of the filesystem while browsing, not to police what may be sent — anything
 * that gets past the dialog is still just bytes to the engine.
 */
export const MEDIA_ACCEPT = 'image/*,video/*';

/** The widest an image is drawn inside a bubble, in CSS pixels. */
const MAX_IMAGE_WIDTH = 300;

/**
 * The tallest an image is drawn inside a bubble.
 *
 * A panorama scaled to 300px wide is still short, but a tall screenshot scaled
 * to 300px wide can be several thousand pixels of bubble. Both limits are
 * applied as a single uniform scale, so the aspect ratio survives either way.
 */
const MAX_IMAGE_HEIGHT = 420;

/**
 * A file staged in the composer but not yet sent.
 *
 * `bytes` is the whole file, already read: the clipboard and the drag data
 * transfer are both valid only for the duration of their event, so there is no
 * later opportunity to go back and read it.
 */
export interface PendingAttachment {
  id: string;
  name: string;
  mimeType: string;
  size: number;
  /**
   * The whole file, for anything small enough to embed.
   *
   * Empty when {@link path} is set: a file streamed from disk is never read
   * into the renderer at all, which is the entire point of streaming it.
   */
  bytes: Uint8Array;
  /** Where the file is on disk, for one too large to embed. */
  path?: string;
  /** An object URL for images only, and only until the attachment goes away. */
  previewUrl?: string;
}

/**
 * An attachment as a bubble renders it: the engine's record, plus a URL for the
 * bytes once they are on this device.
 *
 * `url` is optional so a message's own `attachments` are assignable unchanged.
 * That is not a convenience — the engine does not drive file transfer yet, so
 * for anything that arrived over the wire there genuinely is no URL, and the
 * list shows the transfer's progress rather than pretending otherwise.
 */
export type BubbleAttachment = Attachment & { url?: string };

/** Options for {@link useAttachmentIntake}. */
export interface AttachmentIntakeOptions {
  /**
   * Called when a file is refused, in addition to the hook's own `error`
   * state, so a rejection can also be surfaced in the window's error banner.
   */
  onError?: (message: string) => void;
}

/** Everything {@link useAttachmentIntake} hands back to the composer. */
export interface AttachmentIntake {
  /** The files staged so far, oldest first. */
  attachments: readonly PendingAttachment[];
  /** True while a drag carrying files is over the composer. */
  dropActive: boolean;
  /** The most recent rejection, or null. Cleared by the next successful add. */
  error: string | null;
  onDragOver: (event: React.DragEvent<HTMLElement>) => void;
  onDragLeave: (event: React.DragEvent<HTMLElement>) => void;
  onDrop: (event: React.DragEvent<HTMLElement>) => void;
  /** The hidden file input. It has to be rendered for the picker to open. */
  fileInput: React.ReactElement;
  /**
   * Open the system file picker.
   *
   * `accept` is an `<input accept>` list, narrowing what the dialog offers —
   * "Photos & Videos" and "File" are the same picker with and without one.
   * Omitting it accepts anything.
   */
  openFilePicker: (accept?: string) => void;
  /** Drop one staged attachment, revoking its preview. */
  remove: (id: string) => void;
  /** Drop all of them, revoking every preview. Call this after sending. */
  clear: () => void;
}

/**
 * Turn one chosen file into something the composer can hold, or a refusal.
 *
 * Exported for tests, and separate from the hook because the interesting part
 * is a decision rather than any state: whether this file is small enough to
 * carry inside the message, or has to be streamed from where it already is.
 *
 * `pathFor` is how a `File` is resolved to a path on disk. It is injected
 * rather than reached for, because only the preload can answer it — Electron
 * removed `File.path` — and a test has no preload.
 */
export async function stageFile(
  file: File,
  pathFor: (file: File) => string,
): Promise<PendingAttachment | { error: string }> {
  const mimeType = file.type || 'application/octet-stream';
  const name = file.name || defaultName(mimeType);

  /*
   * Past the embedding limit the file is sent from where it is.
   *
   * Reading it would mean holding all of it in the renderer, which is the very
   * thing streaming exists to avoid — so nothing is read, no preview is built,
   * and the engine hashes it a chunk at a time straight off the disk.
   *
   * A file with no path cannot take this route. That is a pasted screenshot or
   * a dropped blob: it only ever existed in memory, so there is nothing to
   * stream from and the limit genuinely applies.
   */
  if (file.size > EMBEDDED_FILE_LIMIT) {
    // An older bridge has no way to answer this. Staging by path anyway would
    // hand the engine an attachment with neither bytes nor a path, which it
    // rejects with a message about an empty file — true, and no help at all.
    const path = typeof pathFor === 'function' ? pathFor(file) : '';
    if (!path) {
      return {
        error: `${file.name || 'That item'} is ${fileSize(file.size)} and has no file on disk. Save it first, then attach it.`,
      };
    }

    return { id: crypto.randomUUID(), name, mimeType, size: file.size, bytes: new Uint8Array(0), path };
  }

  const bytes = new Uint8Array(await file.arrayBuffer());

  return {
    id: crypto.randomUUID(),
    name,
    mimeType,
    size: file.size,
    bytes,
    // The preview is built from the copy we already hold rather than from the
    // File, so it keeps working if the original is moved or deleted between
    // staging and sending.
    ...(isImageType(mimeType)
      ? { previewUrl: URL.createObjectURL(new Blob([bytes], { type: mimeType })) }
      : {}),
  };
}

/**
 * Wire up the three ways a file gets into a message.
 *
 * Pasting is the one that matters most in practice — screenshot, Cmd-V, send —
 * and it is the most fragile, for two separate reasons.
 *
 * The clipboard's `DataTransferItem`s are only readable inside the handler
 * itself, so every `getAsFile` has to happen before the first await.
 *
 * And the paste has to be caught on the *document*, not on the composer. A
 * paste event targets whatever has focus, and the realistic way to paste a
 * screenshot is to take one, click the window to bring it forward, and hit
 * Cmd-V — at which point focus is on whatever you happened to click, and a
 * listener bound to the composer subtree never hears about it. Attaching from
 * the file picker worked; pasting silently did nothing unless you had first
 * clicked into the text box.
 */
export function useAttachmentIntake(
  options: AttachmentIntakeOptions = {},
): AttachmentIntake {
  const [attachments, setAttachments] = React.useState<PendingAttachment[]>([]);
  const [dropActive, setDropActive] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const inputRef = React.useRef<HTMLInputElement>(null);

  // The callback is read through a ref so that none of the handlers below have
  // to list it as a dependency. They are handed to a memoised composer, and an
  // identity that changed whenever the parent re-rendered would defeat that.
  const onErrorRef = React.useRef(options.onError);
  React.useEffect(() => {
    onErrorRef.current = options.onError;
  }, [options.onError]);

  // Removal and unmount both need the current list from outside a render, and
  // reading it from a ref keeps the state updaters pure — revoking inside one
  // would fire twice under StrictMode's double invocation.
  const attachmentsRef = React.useRef<readonly PendingAttachment[]>(attachments);
  React.useEffect(() => {
    attachmentsRef.current = attachments;
  }, [attachments]);

  // Closing the window is not the only way this unmounts; switching away from a
  // conversation does too, and that is the common case.
  React.useEffect(() => {
    return () => {
      for (const attachment of attachmentsRef.current) {
        if (attachment.previewUrl) URL.revokeObjectURL(attachment.previewUrl);
      }
    };
  }, []);

  const addFiles = React.useCallback(async (files: readonly File[]) => {
    const accepted: PendingAttachment[] = [];
    let rejection: string | null = null;

    for (const file of files) {
      const staged = await stageFile(file, (target) =>
        // Optional chaining, not a bare call: a renderer newer than the
        // preload it is running against should refuse the file, not throw.
        window.bounce.pathForFile?.(target) ?? '',
      );
      if ('error' in staged) rejection = staged.error;
      else accepted.push(staged);
    }

    if (accepted.length > 0) {
      setAttachments((current) => [...current, ...accepted]);
    }

    setError(rejection);
    if (rejection) onErrorRef.current?.(rejection);
  }, []);

  // Bound to the document, so a paste is caught wherever focus happens to be.
  // Text pastes are never touched: the handler returns before doing anything
  // unless the clipboard is actually carrying files.
  React.useEffect(() => {
    const onPaste = (event: ClipboardEvent) => {
      const items = event.clipboardData?.items;
      if (!items) return;

      // A dialog covers the composer, and staging into something the user
      // cannot see would be worse than ignoring the paste.
      if (document.querySelector('.modal__backdrop')) return;

      // Every `getAsFile` has to happen before the first await: the item list
      // is emptied as soon as the handler returns, so a version of this that
      // awaited inside the loop would find the clipboard already gone.
      const files: File[] = [];
      for (const item of Array.from(items)) {
        if (item.kind !== 'file') continue;
        const file = item.getAsFile();
        if (file) files.push(file);
      }

      if (files.length === 0) return;

      // Only swallow the paste once we know we took something from it.
      event.preventDefault();
      void addFiles(files);
    };

    document.addEventListener('paste', onPaste);
    return () => document.removeEventListener('paste', onPaste);
  }, [addFiles]);

  const onDragOver = React.useCallback((event: React.DragEvent<HTMLElement>) => {
    if (!carriesFiles(event.dataTransfer)) return;
    // Without this the window navigates to the dropped file, which in a
    // packaged app replaces the entire interface with a picture and no way back.
    event.preventDefault();
    event.dataTransfer.dropEffect = 'copy';
    setDropActive(true);
  }, []);

  const onDragLeave = React.useCallback((event: React.DragEvent<HTMLElement>) => {
    // dragleave also fires when the pointer crosses onto a child, so a
    // departure that lands somewhere still inside the zone is not a departure.
    const entered = event.relatedTarget;
    if (entered instanceof Node && event.currentTarget.contains(entered)) return;
    setDropActive(false);
  }, []);

  const onDrop = React.useCallback(
    (event: React.DragEvent<HTMLElement>) => {
      event.preventDefault();
      setDropActive(false);
      const files = Array.from(event.dataTransfer?.files ?? []);
      if (files.length > 0) void addFiles(files);
    },
    [addFiles],
  );

  const openFilePicker = React.useCallback((accept?: string) => {
    const input = inputRef.current;
    if (!input) return;
    // Written straight onto the node rather than held in state: the click has
    // to happen in the same turn as the user's, or macOS treats the dialog as
    // unsolicited, and a state update would not have rendered by then.
    input.accept = accept ?? '';
    input.click();
  }, []);

  const remove = React.useCallback((id: string) => {
    const going = attachmentsRef.current.find((attachment) => attachment.id === id);
    if (going?.previewUrl) URL.revokeObjectURL(going.previewUrl);
    setAttachments((current) => current.filter((attachment) => attachment.id !== id));
  }, []);

  const clear = React.useCallback(() => {
    for (const attachment of attachmentsRef.current) {
      if (attachment.previewUrl) URL.revokeObjectURL(attachment.previewUrl);
    }
    setAttachments([]);
    setError(null);
  }, []);

  const fileInput = (
    <input
      ref={inputRef}
      type="file"
      multiple
      hidden
      onChange={(event) => {
        const files = Array.from(event.target.files ?? []);
        // Reset first, so choosing the same file twice in a row still fires a
        // change event the second time.
        event.target.value = '';
        if (files.length > 0) void addFiles(files);
      }}
    />
  );

  return {
    attachments,
    dropActive,
    error,
    onDragOver,
    onDragLeave,
    onDrop,
    fileInput,
    openFilePicker,
    remove,
    clear,
  };
}

/**
 * The staged attachments, shown above the composer.
 *
 * Images are thumbnails because that is how you recognise the screenshot you
 * meant to send; everything else is a chip, because a filename is all there is
 * to recognise it by.
 */
export function AttachmentTray({
  attachments,
  onRemove,
}: {
  attachments: readonly PendingAttachment[];
  onRemove: (id: string) => void;
}) {
  if (attachments.length === 0) return null;

  return (
    <div className="attachment-tray" aria-label="Attachments to send">
      {attachments.map((attachment) => (
        <div className="attachment-tray__item" key={attachment.id}>
          {attachment.previewUrl ? (
            <img
              className="attachment-tray__thumb"
              src={attachment.previewUrl}
              alt={attachment.name}
              draggable={false}
            />
          ) : (
            <div className="attachment-tray__chip">
              <FileIcon />
              <div className="attachment-tray__chip-text">
                <div className="attachment-tray__name" title={attachment.name}>
                  {attachment.name}
                </div>
                <div className="attachment-tray__size">{fileSize(attachment.size)}</div>
              </div>
            </div>
          )}

          <button
            className="attachment-tray__remove"
            onClick={() => onRemove(attachment.id)}
            title={`Remove ${attachment.name}`}
            aria-label={`Remove ${attachment.name}`}
          >
            <CloseIcon size={12} />
          </button>
        </div>
      ))}
    </div>
  );
}

/**
 * The attachments on a message, rendered inside its bubble.
 *
 * An image whose bytes are not here yet still occupies its final size, because
 * the engine sends the dimensions ahead of the pixels. Reserving the space
 * stops the timeline jumping under the reader when the picture lands.
 */
export function AttachmentList({
  attachments,
  onOpenImage,
}: {
  attachments: readonly BubbleAttachment[];
  onOpenImage: (src: string, alt: string) => void;
}) {
  if (attachments.length === 0) return null;

  return (
    <div className="attachment-list">
      {attachments.map((attachment) =>
        isImageAttachment(attachment) ? (
          <ImageAttachmentView
            key={attachment.id}
            attachment={attachment}
            onOpenImage={onOpenImage}
          />
        ) : (
          <FileAttachmentView key={attachment.id} attachment={attachment} />
        ),
      )}
    </div>
  );
}

function ImageAttachmentView({
  attachment,
  onOpenImage,
}: {
  attachment: BubbleAttachment;
  onOpenImage: (src: string, alt: string) => void;
}) {
  const box = displayedSize(attachment.width, attachment.height);
  const url = attachment.url;

  // The blur is decoded once per hash and kept, because a timeline redraw
  // during a download would otherwise re-run the decode on every frame.
  const placeholder = React.useMemo(
    () => (attachment.blurHash ? blurHashToDataUrl(attachment.blurHash) : null),
    [attachment.blurHash],
  );

  if (!url) {
    return (
      <div
        className="attachment-image attachment-image--pending"
        style={{
          ...box,
          // A recognisable blur of the picture, at its final size, so nothing
          // moves when the real pixels replace it.
          ...(placeholder
            ? {
                backgroundImage: `url(${placeholder})`,
                backgroundSize: 'cover',
                backgroundPosition: 'center',
              }
            : null),
        }}
        aria-label={`${attachment.name}, ${transferLabel(attachment.progress)}`}
      >
        <span
          className={
            placeholder
              ? 'attachment-image__progress attachment-image__progress--over-blur'
              : 'attachment-image__progress'
          }
        >
          {transferLabel(attachment.progress)}
        </span>
      </div>
    );
  }

  return (
    <button
      className="attachment-image"
      style={box}
      onClick={() => onOpenImage(url, attachment.name)}
      title={attachment.name}
      aria-label={`Open ${attachment.name}`}
    >
      <img src={url} alt={attachment.name} draggable={false} />
    </button>
  );
}

function FileAttachmentView({ attachment }: { attachment: BubbleAttachment }) {
  return (
    <div className="attachment-file">
      <div className="attachment-file__icon">
        <FileIcon size={22} />
      </div>

      <div className="attachment-file__text">
        <div className="attachment-file__name" title={attachment.name}>
          {attachment.name}
        </div>
        <div className="attachment-file__size">{fileSize(attachment.size)}</div>
      </div>

      {attachment.url ? (
        // An anchor rather than a button: `download` against a blob URL is
        // handled by the browser itself, so saving needs no privileged call.
        <a
          className="attachment-file__download"
          href={attachment.url}
          download={attachment.name}
          title={`Save ${attachment.name}`}
          aria-label={`Save ${attachment.name}`}
        >
          <DownloadIcon />
        </a>
      ) : (
        <span className="attachment-file__progress">
          {transferLabel(attachment.progress)}
        </span>
      )}
    </div>
  );
}

/**
 * One image, full screen.
 *
 * Escape and a click on the backdrop both dismiss it; a click on the picture
 * does not, because dragging or right-clicking a photo is the reason to have
 * opened it.
 */
export function ImageViewer({
  src,
  alt,
  onClose,
}: {
  src: string;
  alt: string;
  onClose: () => void;
}) {
  React.useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    // On the window rather than the overlay: the overlay is not focused when it
    // opens, and Escape has to work without a click first.
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose]);

  return (
    <div
      className="image-viewer"
      role="dialog"
      aria-modal="true"
      aria-label={alt}
      onClick={onClose}
    >
      <button
        className="image-viewer__close"
        onClick={onClose}
        title="Close"
        aria-label="Close"
      >
        <CloseIcon />
      </button>

      <img
        className="image-viewer__image"
        src={src}
        alt={alt}
        onClick={(event) => event.stopPropagation()}
      />
    </div>
  );
}

/**
 * Whether an attachment is shown as a picture.
 *
 * The engine records pixel dimensions for image attachments and leaves them
 * null for file attachments, so that is the whole test — no sniffing of names
 * or bytes, and in particular nothing that could talk the renderer into
 * treating an SVG, which is a script-bearing document rather than a bitmap, as
 * an image from a stranger.
 */
function isImageAttachment(attachment: BubbleAttachment): boolean {
  return attachment.width !== null && attachment.height !== null;
}

/** Whether a MIME type from the clipboard or a drop is worth previewing. */
function isImageType(mimeType: string): boolean {
  // Same reasoning as `isImageAttachment`: an SVG is a document, not a picture.
  return mimeType.startsWith('image/') && mimeType !== 'image/svg+xml';
}

/** A name for a clipboard blob, which usually arrives without one. */
function defaultName(mimeType: string): string {
  const subtype = mimeType.split('/')[1] ?? '';
  const extension = subtype.replace(/[^a-z0-9]/gi, '') || 'bin';
  return `pasted-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, '')}.${extension}`;
}

/** Whether a drag is carrying files rather than text from another window. */
function carriesFiles(transfer: DataTransfer | null): boolean {
  return transfer !== null && Array.from(transfer.types).includes('Files');
}

/**
 * The box an image occupies in a bubble, scaled down to fit both limits.
 *
 * Undefined when the dimensions are unknown, which leaves the CSS max-width to
 * do the same job once the image has decoded.
 */
function displayedSize(
  width: number | null,
  height: number | null,
): { width: number; height: number } | undefined {
  if (!width || !height) return undefined;
  const scale = Math.min(MAX_IMAGE_WIDTH / width, MAX_IMAGE_HEIGHT / height, 1);
  return { width: Math.round(width * scale), height: Math.round(height * scale) };
}

/** How far along a transfer is, for an attachment whose bytes are not here. */
function transferLabel(progress: number): string {
  if (progress <= 0) return 'Not downloaded';
  return `${Math.round(progress * 100)}%`;
}

/*
 * Two glyphs the shared icon set does not carry, drawn on the same 20px grid
 * with the same 1.7px stroke so they sit correctly beside the icons from
 * `icons.tsx`.
 */

/** A generic document, for an attachment that is not a picture. */
function FileIcon({ size = 20 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M11.4 2.6H5.8a1.6 1.6 0 0 0-1.6 1.6v11.6a1.6 1.6 0 0 0 1.6 1.6h8.4a1.6 1.6 0 0 0 1.6-1.6V6.8z" />
      <path d="M11.4 2.6v4.2h4.4" />
    </svg>
  );
}

/** Save an attachment to disk. */
function DownloadIcon({ size = 18 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M10 3.4v8.6" />
      <path d="M6.4 8.6 10 12.2 13.6 8.6" />
      <path d="M4 14.4v1.2a1.4 1.4 0 0 0 1.4 1.4h9.2a1.4 1.4 0 0 0 1.4-1.4v-1.2" />
    </svg>
  );
}
