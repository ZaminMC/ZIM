// The panel's icons — ONE system, Lucide (https://lucide.dev, ISC
// license), for every chrome, content, and popup surface. The glyphs are
// upstream's; the names are the panel's own vocabulary, so a view says
// "IconPlayers" rather than lucide's "Users" and a swap of the underlying
// system never touches a view. One grid (24), one weight (2), one color
// rule (currentColor), one default render size (16, the UI's other small
// marks). The house mark (IconZim) is the one hand-drawn exception: a
// brand glyph, not a system icon.

import type { CSSProperties } from "react";
import type { LucideIcon } from "lucide-react";
import {
  Activity,
  AppWindow,
  ArrowLeft,
  ArrowRight,
  Blocks,
  Check,
  ChevronLeft,
  ChevronRight,
  Clock,
  Copy,
  DatabaseBackup,
  Download,
  Ellipsis,
  Folder,
  Globe,
  Group,
  History,
  Info,
  LayoutDashboard,
  LayoutGrid,
  ListChecks,
  MessageSquare,
  MessageSquarePlus,
  Minus,
  Network,
  Pin,
  Plus,
  Puzzle,
  Rocket,
  RotateCcw,
  RotateCw,
  ScrollText,
  Search,
  Server,
  Settings,
  ShieldCheck,
  Sparkles,
  Square,
  SquareArrowOutUpRight,
  SquareCode,
  Star,
  Sunrise,
  Terminal,
  Users,
  Ungroup,
  VolumeX,
  X,
  Zap,
  ZoomIn,
  ZoomOut,
} from "lucide-react";

interface IconProps {
  size?: number;
  className?: string;
  style?: CSSProperties;
}

/** Wrap a Lucide glyph under the panel's props: 16 default, caller's
 *  className/style ride through, decorative by default (aria-hidden) —
 *  a caller that labels the icon can override with its own attribute. */
function from(Glyph: LucideIcon) {
  return function PanelIcon(p: IconProps) {
    return (
      <Glyph
        size={p.size ?? 16}
        className={p.className}
        style={p.style}
        aria-hidden="true"
      />
    );
  };
}

export const IconActivity = from(Activity);
export const IconApps = from(LayoutGrid);
export const IconArrowLeft = from(ArrowLeft);
export const IconArrowRight = from(ArrowRight);
export const IconAudit = from(History);
export const IconBack = from(ChevronLeft);
export const IconBackups = from(DatabaseBackup);
export const IconBolt = from(Zap);
export const IconChat = from(MessageSquare);
export const IconCheck = from(Check);
export const IconChevLeft = from(ChevronLeft);
export const IconChevRight = from(ChevronRight);
export const IconClock = from(Clock);
export const IconClose = from(X);
export const IconCopy = from(Copy);
export const IconDashboard = from(LayoutDashboard);
export const IconDevTools = from(SquareCode);
export const IconDots = from(Ellipsis);
export const IconDownload = from(Download);
export const IconDuplicate = from(Copy);
export const IconExtension = from(Blocks);
export const IconFeedback = from(MessageSquarePlus);
export const IconFolder = from(Folder);
export const IconForward = from(ChevronRight);
export const IconGear = from(Settings);
export const IconGlobe = from(Globe);
export const IconGroup = from(Group);
export const IconInfo = from(Info);
export const IconJobs = from(ListChecks);
export const IconLogs = from(ScrollText);
export const IconMaximize = from(Square);
export const IconMinimize = from(Minus);
export const IconMuted = from(VolumeX);
export const IconNetwork = from(Network);
export const IconNewWindow = from(AppWindow);
export const IconOpenInNew = from(SquareArrowOutUpRight);
export const IconPlayers = from(Users);
export const IconUngroup = from(Ungroup);
export const IconPin = from(Pin);
export const IconPlus = from(Plus);
export const IconPuzzle = from(Puzzle);
export const IconReload = from(RotateCw);
export const IconRocket = from(Rocket);
export const IconSearch = from(Search);
export const IconServer = from(Server);
export const IconShield = from(ShieldCheck);
export const IconSparkles = from(Sparkles);
export const IconStar = from(Star);
export const IconTerminal = from(Terminal);
export const IconVolumeMuted = from(VolumeX);
export const IconWake = from(Sunrise);
export const IconWindowClose = from(X);
export const IconZoomIn = from(ZoomIn);
export const IconZoomOut = from(ZoomOut);
export const IconZoomReset = from(RotateCcw);

export type { IconProps };

/** The house mark: the Z bolt, the new-tab page's own glyph. Hand-drawn
 *  on its own 16 box — a brand mark answers to the product, not to the
 *  system grid, and is the one icon in the panel upstream does not own. */
export function IconZim(p: IconProps) {
  return (
    <svg
      width={p.size ?? 16}
      height={p.size ?? 16}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={p.className}
      style={p.style}
      aria-hidden="true"
    >
      <path d="M4.2 3.5h7.6L4.2 12.5h7.6" />
    </svg>
  );
}
