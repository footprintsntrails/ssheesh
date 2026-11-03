// Which files the preview in Files shows as a picture or plays as a video.
// What this computer can't decode says so when it tries.

export type MediaKind = "image" | "video";

const IMAGE = /\.(png|jpe?g|gif|webp|svg|ico|bmp|avif|heic|tiff?)$/i;
const VIDEO = /\.(mp4|m4v|mov|webm|ogv|mkv|avi)$/i;
/** A picture that's text too: it can be switched to its text, to edit it. */
export const SVG = /\.svg$/i;

/** A picture this big would take long to come, and more memory than it's worth: see `MAX_WHOLE` in media.rs. */
export const MAX_PICTURE = 128 * 1024 * 1024;

export function mediaKind(name: string): MediaKind | null {
  if (IMAGE.test(name)) return "image";
  return VIDEO.test(name) ? "video" : null;
}
