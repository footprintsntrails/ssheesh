// A server's icon, the same everywhere it's drawn: your own picture if you
// gave it one, else its icon in its color.

import { cn } from "../lib/format";
import { isHostImage, LOCAL_GLYPH, useHostLook, type ServerGlyph } from "../lib/labels";
import { useSettings } from "../lib/settings";

/**
 * A look as an icon, `size` pixels square: the picture, rounded and as it
 * is, else the icon in its color (the accent color without one). Streamer
 * mode always shows the icon: a picture could give the server away.
 */
export function LookIcon({
  look,
  size,
  strokeWidth = 1.9,
  className,
}: {
  look: { icon: ServerGlyph; color: string | null; image?: string | null };
  size: number;
  strokeWidth?: number;
  className?: string;
}) {
  const streamer = useSettings((s) => s.streamerMode);
  if (!streamer && isHostImage(look.image))
    return (
      <img
        src={look.image}
        alt=""
        draggable={false}
        width={size}
        height={size}
        className={cn("shrink-0 rounded-[22%] object-cover", className)}
        style={{ width: size, height: size }}
      />
    );
  const Icon = look.icon;
  return <Icon size={size} strokeWidth={strokeWidth} className={cn(!look.color && "text-accent", className)} style={look.color ? { color: look.color } : undefined} />;
}

/** A server's icon or picture. `local` draws This computer's laptop in the accent color. */
export function HostIcon({
  hostId,
  local = false,
  size,
  strokeWidth,
  className,
}: {
  hostId: string | undefined;
  local?: boolean;
  size: number;
  strokeWidth?: number;
  className?: string;
}) {
  const look = useHostLook(local ? undefined : hostId);
  return <LookIcon look={local ? { icon: LOCAL_GLYPH, color: null } : look} size={size} strokeWidth={strokeWidth} className={className} />;
}
