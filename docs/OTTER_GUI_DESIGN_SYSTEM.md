# Otter GUI: design system specification

Date: 2026-11-06  
Document version: 1.0.0  
Design reference: Four-screen Otter GUI concept  

---

## 1. Visual identity and design principles

The Otter desktop interface is designed as a calm, approachable consumer desktop application. It avoids industrial server dashboards, high-contrast neon themes, and cluttered telemetry widgets.

Key characteristics:
- Warm white and ivory base background.
- Restrained soft pink accents for primary interactive elements and active navigation states.
- Dark, readable typography with generous whitespace.
- Generous rounded corners on cards, badges, and buttons.
- Thin borders with soft ambient drop shadows.
- Dedicated mascot illustrations positioned thoughtfully across views.

---

## 2. Color palette

| Token | Value | Role |
| :--- | :--- | :--- |
| `--bg-app` | `#FAF7F5` | Main application background (warm ivory) |
| `--bg-card` | `#FFFFFF` | Content card background |
| `--bg-card-hover` | `#FCFAF8` | Card hover state |
| `--bg-sidebar` | `#FAF7F5` | Sidebar background |
| `--bg-pill-active` | `#FFEFF4` | Active navigation pill background |
| `--accent-pink` | `#F472B6` | Primary brand accent and buttons |
| `--accent-pink-hover` | `#EC4899` | Button hover state |
| `--accent-pink-light` | `#FDF2F8` | Light pink badge background |
| `--text-primary` | `#1F1A24` | Main headings and primary content |
| `--text-secondary` | `#6B6375` | Subtitles, labels, and metadata |
| `--text-muted` | `#9E97A6` | Placeholders and inactive icons |
| `--border-subtle` | `#F0EAE6` | Card and divider borders |
| `--border-focus` | `#F472B6` | Input focus outline |
| `--status-active` | `#F472B6` | Active progress fill |
| `--status-completed` | `#10B981` | Completed checkmark and green pill |
| `--status-paused` | `#F59E0B` | Paused amber indicator |
| `--status-error` | `#EF4444` | Error indicator |

---

## 3. Typography

- Font family: `Segoe UI`, `Inter`, `-apple-system`, `BlinkMacSystemFont`, sans-serif.
- Headings:
  - App title: 18px, font-weight 700.
  - Page title: 24px, font-weight 700, letter-spacing -0.02em. Includes a soft pink bullet dot at the end (`Title .`).
  - Section title: 16px, font-weight 600.
- Body text:
  - Primary body: 14px, font-weight 400, line-height 1.5.
  - Subtitle / descriptions: 13px, font-weight 400, color `--text-secondary`.
  - Small labels and metadata: 12px, font-weight 500.
  - Badges and hotkeys: 11px, font-weight 600.

---

## 4. Spacing and layout geometry

- Application window default: 1120px width by 740px height (minimum: 960px by 640px).
- Sidebar: fixed width of 200px.
- Main content area: fluid width with padding of 24px to 32px.
- Border radius:
  - Cards: 16px.
  - Buttons and inputs: 12px.
  - Navigation items: 12px.
  - Filter pills and badges: 9999px (full pill).
  - Progress bars: 8px.
- Shadows:
  - Card shadow: `0 2px 8px rgba(0, 0, 0, 0.03), 0 1px 2px rgba(0, 0, 0, 0.02)`.
  - Floating modal shadow: `0 12px 32px rgba(0, 0, 0, 0.08)`.

---

## 5. Mascot illustration placement

All four official mascot illustrations from `images/` are integrated in their designated contexts:

1. Overview hero (top-right):  
   `images/Flying Otter Mascot with Flowing Scarf.png`  
   Positioned next to the "Welcome back, Aaryan" greeting card.
2. Downloads (bottom-right):  
   `images/Focused Otter with Laptop and Coffee.png`  
   Positioned in the lower corner of the active downloads view.
3. Activity (bottom-left):  
   `images/Cheerful Otter Popping Out of Pink Box.png`  
   Positioned at the base of the app activity feed.
4. Settings (top-right):  
   `images/Sleeping Otter with Pink Paw Pillow.png`  
   Positioned comfortably above the settings panels.

---

## 6. Component catalog

### Sidebar navigation
Vertical item list:
- Overview (dashboard icon)
- Downloads (download tray icon with numeric count badge)
- Activity (signal wave icon)
- Library (bookshelf/folder icon)
- Engine (cpu/gear icon)
- Settings (gear icon)

### Summary cards
Row of four cards on the Overview screen:
- Active downloads (count, download icon)
- Completed downloads (count, checkmark icon)
- Total speed (live throughput in MB/s, lightning bolt icon)
- Downloaded data (total gigabytes transferred, database icon)

### Download row
Card item showing:
- Platform / source icon (Hugging Face, Steam, Epic Games, Chrome, direct file).
- Filename (bold, 14px).
- Metadata subtitle (Source platform, file size).
- Progress bar with soft pink fill.
- Live progress percentage and speed (e.g. `67% • 92.4 MB/s`).
- Pause / resume control button.
- Context menu button (`⋮`).

### Universal URL input
Prominent input bar at the top of the Downloads view:
- Search / URL input with icon and placeholder `Paste any URL here...`.
- Clear button.
- Primary pink `Add Download` action button.
- Detection preview modal displaying detected platform, content type, title, size, and download confirmation.
