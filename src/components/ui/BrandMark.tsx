import { useId } from "react";

/**
 * The SayQL logo: a Q whose tail is a speech-bubble tail. Same artwork as
 * src-tauri/icons/app-icon.svg, cropped to the tile (no macOS safe-area margin).
 */
export function BrandMark({ size = 18, className }: { size?: number; className?: string }) {
  const gradient = useId();
  return (
    <svg width={size} height={size} viewBox="100 100 824 824" className={className} aria-hidden="true">
      <defs>
        <linearGradient id={gradient} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#4F46E5" />
          <stop offset="1" stopColor="#7C3AED" />
        </linearGradient>
      </defs>
      <rect x="100" y="100" width="824" height="824" rx="185" fill={`url(#${gradient})`} />
      <circle cx="496" cy="478" r="196" fill="none" stroke="#FFFFFF" strokeWidth="96" />
      <path d="M676 590 C 700 670 742 728 800 772 C 716 770 646 740 594 690 Z" fill="#FFFFFF" />
    </svg>
  );
}
