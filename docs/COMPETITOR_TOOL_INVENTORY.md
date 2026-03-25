# Wikipedia Patrol & Anti-Vandalism Tools: Comprehensive Feature Inventory

_Research conducted March 2026. Sources include MediaWiki.org, Wikipedia project pages, GitHub/GitLab repositories, Wikimedia Diff blog, Wikitech, and Meta-Wiki._

---

## Table of Contents

1. [Huggle](#1-huggle)
2. [LiveRC](#2-liverc)
3. [SWViewer](#3-swviewer)
4. [Twinkle](#4-twinkle)
5. [RedWarn / Ultraviolet](#5-redwarn--ultraviolet)
6. [RTRC (Real-Time Recent Changes)](#6-rtrc-real-time-recent-changes)
7. [AntiVandal](#7-antivandal)
8. [STiki](#8-stiki)
9. [Igloo](#9-igloo)
10. [ClueBot NG](#10-cluebot-ng)
11. [ORES / LiftWing](#11-ores--liftwing)
12. [Patroller (MediaWiki Extension)](#12-patroller-mediawiki-extension)
13. [WikiPatrol / Wikipedia Android Edit Patrol](#13-wikipatrol--wikipedia-android-edit-patrol)
14. [Feature Comparison Matrix](#14-feature-comparison-matrix)

---

## 1. Huggle

**Current Status:** Active (v3.4.14, released November 2025)
**Platform:** Desktop application (Windows 10+, macOS, Linux/Debian/Ubuntu)
**Technology:** C++ (94.1%), Qt5/Qt6 framework, CMake build system
**License:** GPL v3+
**Source:** [github.com/huggle/huggle3-qt-lx](https://github.com/huggle/huggle3-qt-lx)
**Requirements:** Rollback permission required on English Wikipedia

### Features

| Feature | Supported | Notes |
|---------|-----------|-------|
| Real-time edit feed | Yes | Uses IRC recent changes feed or MediaWiki API as "providers" |
| Edit queue with scoring/prioritization | Yes | Edits pre-parsed and analyzed; sorted by predicted vandalism level |
| Diff viewing | Yes | Core feature -- fast diff browser with WebKit or WebEngine/Chromium backend |
| One-click rollback | Yes | Shortcut key "Q" for quick revert |
| One-click revert/undo | Yes | Multiple revert options available |
| Patrol marking | Yes | Ctrl+P marks new pages as patrolled |
| User warning | Yes | Automated warnings based on latest warning level on user talk page |
| User talk page integration | Yes | Direct message posting to user talk pages |
| ML/AI scoring (ORES/LiftWing) | Yes | ORES integration with configurable thresholds (ores-enabled, ores-amplifier) |
| Multi-wiki support | Yes | Works on any MediaWiki wiki; list of supported Wikimedia projects maintained |
| Coordination between users | Partial | Semi-distributed model; global whitelist shared across users |
| Offline capability | No | Requires live connection to wiki |
| Customizable filters | Yes | Namespace, bots, self-edits, talk pages, page types |
| Whitelist/trusted user management | Yes | Global whitelist + local user-badness scores (self-learning) |
| Training data export | No | Not documented |
| Edit summary customization | Yes | Configurable edit summaries |
| Watchlist integration | Partial | Can interact with watchlist |
| Page protection requests | No | Not a core feature |
| Reporting to administrators | Yes | Supports AIV/UAA reporting |
| Block requests | Partial | Through reporting workflows |
| Tag management | No | Not a core feature |
| Keyboard shortcuts | Yes | Fully customizable via System > Options > Keyboard tab |
| Mobile support | No | Desktop-only application |
| Installable/PWA | N/A | Native desktop application |
| Dark mode / theming | No | No dark mode documented as of 2025 |

### Distinctive Features
- Self-learning user reputation scoring stored locally
- Multiple edit providers (API, IRC)
- WebKit or Chromium rendering engine options
- 4,352+ commits, 57 releases -- mature codebase
- IRC community support (#huggle on Libera.Chat)

---

## 2. LiveRC

**Current Status:** Legacy/Maintenance (still functional, no active development since ~2014)
**Platform:** Browser gadget (runs inside MediaWiki)
**Technology:** JavaScript (MediaWiki gadget)
**Primary Wiki:** French Wikipedia (fr.wikipedia.org), also used on Wikibooks
**Source:** [MediaWiki:Gadget-LiveRC.js on fr.wiki](https://fr.wikipedia.org/wiki/MediaWiki:Gadget-LiveRC.js)
**Version:** 1.0.5 (per Wikidata)

### Features

| Feature | Supported | Notes |
|---------|-----------|-------|
| Real-time edit feed | Yes | Uses EventStream and MediaWiki API for real-time changes |
| Edit queue with scoring/prioritization | Partial | Logs-based prioritization, not ML scoring |
| Diff viewing | Yes | Inline diff viewing within the interface |
| One-click rollback | Yes | Rollback of last contributor (limited to 10 modifications) |
| One-click revert/undo | Yes | Undo last modification available |
| Patrol marking | Yes | Autopatrol support for qualified users (3 months, 500 edits on frwiki) |
| User warning | Yes | Warning templates (Test 1-4, Avertissement Copyvio) via UserWarnings extension |
| User talk page integration | Yes | Warning posting to talk pages |
| ML/AI scoring (ORES/LiftWing) | No | Predates ORES integration |
| Multi-wiki support | Limited | Primarily French Wikipedia; can be installed on other wikis with configuration |
| Coordination between users | No | No coordination mechanism |
| Offline capability | No | Requires live connection |
| Customizable filters | Yes | Namespace filters, user type filters, abuse filter monitoring |
| Whitelist/trusted user management | Partial | Autopatrolled user recognition |
| Training data export | No | Not supported |
| Edit summary customization | Partial | Predefined summaries |
| Watchlist integration | Yes | Monitors edits on watched pages |
| Page protection requests | No | Not documented |
| Reporting to administrators | Partial | Through standard wiki processes |
| Block requests | No | Not a core feature |
| Tag management | No | Not supported |
| Keyboard shortcuts | Limited | Basic browser-level shortcuts |
| Mobile support | No | Desktop browser only; interface noted as cluttered with small links |
| Installable/PWA | No | MediaWiki gadget |
| Dark mode / theming | No | No theming support |

### Distinctive Features
- Extensible via LiveRC extensions (UserWarnings, ProposeTranslation, etc.)
- Monitors abuse filter detections, spam blacklist hits, new user registrations
- LiveRC 2.0 specifications were drafted but never fully implemented
- Noted issues: obsolete codebase, busy interface, small links

### Why It Stalled
- Code became obsolete as more efficient APIs (EventStreams) emerged
- Interface ergonomics criticized -- very busy layout
- LiveRC 2.0 rewrite was planned but never completed

---

## 3. SWViewer

**Current Status:** Active
**Platform:** Web application (hosted on Toolforge)
**Technology:** PHP (43%), JavaScript (38%), CSS (9.6%), HTML (8.3%)
**URL:** [swviewer.toolforge.org](https://swviewer.toolforge.org)
**Source:** [github.com/SWViewer/tool-swviewer](https://github.com/SWViewer/tool-swviewer)
**555 commits**

### Features

| Feature | Supported | Notes |
|---------|-----------|-------|
| Real-time edit feed | Yes | Uses EventStreams to get edits from all WMF wikis |
| Edit queue with scoring/prioritization | Yes | Global queue (small wikis) and local queue modes |
| Diff viewing | Yes | Edit source viewing |
| One-click rollback | Yes | Rollback with custom or predefined summary |
| One-click revert/undo | Yes | Undo support |
| Patrol marking | Partial | Through rollback actions |
| User warning | Yes | Warning templates (must be configured per-wiki in config page) |
| User talk page integration | Yes | Warning posting |
| ML/AI scoring (ORES/LiftWing) | No | Not documented |
| Multi-wiki support | Yes | Core feature -- monitors edits across all Wikimedia projects |
| Coordination between users | Partial | Global queue shared across stewards/global sysops |
| Offline capability | No | Web-based, requires connection |
| Customizable filters | Yes | Per-wiki configuration, common summaries configurable |
| Whitelist/trusted user management | Partial | Relies on global user rights |
| Training data export | No | Not supported |
| Edit summary customization | Yes | "Rollback with summary" button, configurable common summaries |
| Watchlist integration | No | Not documented |
| Page protection requests | No | Not a core feature |
| Reporting to administrators | Yes | Report requests tracked in statistics |
| Block requests | Partial | Through reporting |
| Tag management | No | Not supported |
| Keyboard shortcuts | Yes | Hotkeys available (can be disrupted by browser focus issues) |
| Mobile support | Yes | Mobile-friendly interface -- explicitly designed for mobile phones |
| Installable/PWA | No | Standard web app |
| Dark mode / theming | No | Not documented |

### Distinctive Features
- Two queue modes: Global (for stewards, global sysops, global rollbackers) and Local (for per-wiki rollbackers)
- Speedy deletion tagging support
- Statistics tracking: rollbacks, undos, speedy delete tags, page edits, warnings, report requests
- Cross-platform by design -- works on any device with a browser

---

## 4. Twinkle

**Current Status:** Active (~49,000 users; 3,267 commits, 246 open issues, actively maintained)
**Platform:** Browser gadget (JavaScript loaded via MediaWiki Gadgets)
**Technology:** JavaScript (98.8%), CSS (1.2%)
**Source:** [github.com/wikimedia-gadgets/twinkle](https://github.com/wikimedia-gadgets/twinkle)
**Requirements:** Autoconfirmed status (4 days, 10 edits)
**Variants:** TwinkleGlobal (cross-wiki fork by Xiplus), Twinkle Lite (Spanish Wikipedia)

### Features

| Feature | Supported | Notes |
|---------|-----------|-------|
| Real-time edit feed | No | Not a feed-based tool; operates on individual pages/diffs |
| Edit queue with scoring/prioritization | No | Action-based, not queue-based |
| Diff viewing | Yes | Operates on diff views, adds action buttons |
| One-click rollback | Yes | Three types: Vandalism rollback (with auto-warning), AGF rollback (friendly), Normal rollback |
| One-click revert/undo | Yes | Rollback and undo options |
| Patrol marking | No | Not a patrol marking tool |
| User warning | Yes | Full library of warning templates (uw-vandalism1-4im, etc.) |
| User talk page integration | Yes | Warnings, welcomes, user talk page messages |
| ML/AI scoring (ORES/LiftWing) | No | Does not integrate ORES directly |
| Multi-wiki support | Partial | English Wikipedia primary; TwinkleGlobal fork provides cross-wiki support for rollback, SRG, speedy deletion |
| Coordination between users | No | Individual tool |
| Offline capability | No | Requires live connection |
| Customizable filters | N/A | Not a filtering tool |
| Whitelist/trusted user management | No | Not a feature |
| Training data export | No | Not supported |
| Edit summary customization | Yes | Customizable via Twinkle preferences panel |
| Watchlist integration | Yes | Pages auto-added to watchlist on CSD nomination; configurable per module |
| Page protection requests | Yes | RPP (Request Page Protection) module |
| Reporting to administrators | Yes | Semi-automatic AIV (Administrator Intervention against Vandalism) reporting |
| Block requests | Yes | Through AIV and admin tools (admin-only: direct block interface) |
| Tag management | Yes | Full maintenance tag library for articles |
| Keyboard shortcuts | Limited | Not extensively documented for Twinkle-specific shortcuts |
| Mobile support | Partial | Works on modern smartphone browsers (Chrome, Firefox) |
| Installable/PWA | No | MediaWiki gadget |
| Dark mode / theming | No | Follows wiki skin theming |

### Distinctive Features
- **Deletion modules:** CSD (speedy deletion), PROD (proposed deletion), XfD (Articles for Deletion, etc.)
- **Admin tools:** Direct page deletion (with talk page and redirect cleanup), block, unblock, protect
- **Batch operations:** Batch PROD deletions, configurable batch size (default/max 50)
- **Welcome module:** Auto-welcome new users with configurable templates
- **Tag module:** Comprehensive article maintenance tagging
- **ARV module:** Semi-automatic vandal reporting
- **DI module:** File deletion requests
- One of the most installed non-default gadgets on English Wikipedia

---

## 5. RedWarn / Ultraviolet

### RedWarn
**Current Status:** Maintenance mode (security updates only)
**Platform:** Browser userscript (JavaScript)
**Technology:** JavaScript
**Source:** [gitlab.wikimedia.org/repos/10nm/redwarn-web](https://gitlab.wikimedia.org/repos/10nm/redwarn-web) (462 commits)
**Requirements:** Autoconfirmed status

### Ultraviolet (Successor)
**Current Status:** In development (not yet feature-complete)
**Platform:** Browser userscript
**Technology:** JavaScript (rewrite from scratch)
**Planned features:** Safari support, mobile usability, i18n, wiki-specific configs, dark mode

### RedWarn Features

| Feature | Supported | Notes |
|---------|-----------|-------|
| Real-time edit feed | Partial | "Alert on Change" monitors specific articles for new edits; not a full feed |
| Edit queue with scoring/prioritization | No | Not queue-based |
| Diff viewing | Yes | Core feature; diff viewing with rollback preview |
| One-click rollback | Yes | ~20 preset quick rollback reasons with auto-warning |
| One-click revert/undo | Yes | Multiple revert options |
| Patrol marking | Yes | Recent changes patrol and pending changes review |
| User warning | Yes | Full warning template library with level detection |
| User talk page integration | Yes | Auto-opens warning dialog after rollback; shows current warning level |
| ML/AI scoring (ORES/LiftWing) | Indirect | Uses ORES through Recent Changes filtering |
| Multi-wiki support | No | English Wikipedia only (Ultraviolet planned for i18n) |
| Coordination between users | No | Individual tool |
| Offline capability | No | Requires live connection |
| Customizable filters | Partial | Preferences for behavior customization |
| Whitelist/trusted user management | No | Not documented |
| Training data export | No | Not supported |
| Edit summary customization | Yes | Auto-filled summaries with rollback reasons |
| Watchlist integration | Partial | Through standard wiki integration |
| Page protection requests | Yes | "Manage Page Protection" dialog for requesting protection/unprotection |
| Reporting to administrators | Yes | Auto-detection of level 4 warnings triggers AIV report dialog |
| Block requests | Yes | Through AIV reporting |
| Tag management | Partial | Through standard editing |
| Keyboard shortcuts | Limited | Not extensively documented |
| Mobile support | No (RedWarn); Planned (Ultraviolet) | Ultraviolet development includes mobile usability |
| Installable/PWA | No | Userscript |
| Dark mode / theming | No (RedWarn); Planned (Ultraviolet) | Ultraviolet has dark mode in development |

### Distinctive Features
- **Multiple Action Tool:** Warn or tag 1-50 editors at once from any history page (extended-confirmed users)
- **Pending Changes Review:** Review pending changes with editor notification
- **Preview Rollback:** Preview the change a rollback would make before executing
- **Page alert monitoring:** Background tab notification when an article is edited
- **Rollback preview:** Unique "preview rollback" button

### Ultraviolet Status
- Rewrite diverged significantly from RedWarn -- essentially a new tool
- Name chosen as electromagnetic opposite of "red" on the spectrum
- RedWarn will be decommissioned once Ultraviolet reaches feature parity
- Not yet backwards-compatible with all RedWarn features

---

## 6. RTRC (Real-Time Recent Changes)

**Current Status:** Active (maintained by Krinkle/Timo Tijhof)
**Platform:** Browser gadget (loads via Special:BlankPage/RTRC)
**Technology:** JavaScript
**Source:** [github.com/wikimedia/mediawiki-gadgets-RTRC](https://github.com/wikimedia/mediawiki-gadgets-RTRC)
**Available on:** Any wiki via gadget installation or Special:BlankPage/RTRC

### Features

| Feature | Supported | Notes |
|---------|-----------|-------|
| Real-time edit feed | Yes | Core feature -- auto-refreshes list silently in background |
| Edit queue with scoring/prioritization | Partial | ORES scoring integration highlights high-probability vandalism |
| Diff viewing | Yes | Auto-diff loading |
| One-click rollback | No | View-only monitoring; actions performed via wiki interface |
| One-click revert/undo | No | Not an action tool |
| Patrol marking | Yes | Core feature -- "Unpatrolled only" checkbox; real-time patrol status updates |
| User warning | No | Not an action tool |
| User talk page integration | No | Not an action tool |
| ML/AI scoring (ORES/LiftWing) | Yes | ORES scores computed for all changes; high-probability edits highlighted |
| Multi-wiki support | Yes | Can monitor any Wikimedia wiki |
| Coordination between users | Yes | Patrolled edits hide immediately for all users -- prevents duplicate patrol work |
| Offline capability | No | Requires live connection |
| Customizable filters | Yes | Patrolled/unpatrolled, user type, change type, namespace, start/end date |
| Whitelist/trusted user management | No | Not a feature |
| Training data export | No | Not supported |
| Edit summary customization | N/A | Not an editing tool |
| Watchlist integration | No | Separate from watchlist |
| Page protection requests | No | Not an action tool |
| Reporting to administrators | No | Not an action tool |
| Block requests | No | Not an action tool |
| Tag management | No | Not supported |
| Keyboard shortcuts | Limited | Standard browser shortcuts |
| Mobile support | Partial | Web-based, may work on mobile browsers |
| Installable/PWA | No | MediaWiki gadget |
| Dark mode / theming | No | Not documented |

### Distinctive Features
- **CVN Integration:** Countervandalism Network blacklist consultation; flagged usernames highlighted
- **Timeframing:** View edits within specific time windows
- **Zero-interruption refresh:** New data pushed seamlessly without page flicker
- **Live patrol deduplication:** Edits patrolled by others disappear in real-time
- **Internationalized:** Translations via translatewiki.net

---

## 7. AntiVandal

**Current Status:** Active (v2.0, released September 2025)
**Platform:** Browser-based userscript (JavaScript)
**Technology:** JavaScript
**Developer:** User:Ingenuity
**Source:** User:Ingenuity/avsource on English Wikipedia
**Requirements:** Rollback permission required

### Features

| Feature | Supported | Notes |
|---------|-----------|-------|
| Real-time edit feed | Yes | Loads edits in real-time |
| Edit queue with scoring/prioritization | Yes | ORES integration shows highest-priority edits first |
| Diff viewing | Yes | Core feature |
| One-click rollback | Yes | Quick revert functionality |
| One-click revert/undo | Yes | Revert support |
| Patrol marking | Partial | Through revert actions |
| User warning | Yes | Warning templates |
| User talk page integration | Yes | Warning posting |
| ML/AI scoring (ORES/LiftWing) | Yes | Integrated with ORES to prioritize edits |
| Multi-wiki support | No | English Wikipedia focused |
| Coordination between users | No | Individual tool |
| Offline capability | No | Requires live connection |
| Customizable filters | Yes | "More customizable settings" added in v2.0 |
| Whitelist/trusted user management | Partial | Through rollback permission system |
| Training data export | No | Not supported |
| Edit summary customization | Yes | Customizable |
| Watchlist integration | Partial | Standard wiki integration |
| Page protection requests | No | Not documented |
| Reporting to administrators | Partial | Through standard processes |
| Block requests | No | Not a core feature |
| Tag management | No | Not supported |
| Keyboard shortcuts | Not documented | May exist but not confirmed |
| Mobile support | Not documented | Browser-based, may work |
| Installable/PWA | No | Userscript |
| Dark mode / theming | Yes | Dark mode added in v2.0 (September 2025) |

### Distinctive Features
- Aims to replicate Huggle features in a browser-based tool
- "More intuitive, modern, and easy to install" compared to Huggle
- v2.0 rewrite added dark mode and improved customization
- No desktop installation required

---

## 8. STiki

**Current Status:** Semi-active (last release v2.1, December 2018; back-end may still be running)
**Platform:** Desktop application (Java)
**Technology:** Java (GUI as executable JAR), back-end server, PostgreSQL database
**Source:** [github.com/westand/STiki](https://github.com/westand/STiki) (73 commits)
**Requirements:** Rollback permission + STiki access approval
**Cumulative Impact:** 1,265,447+ edits reverted

### Features

| Feature | Supported | Notes |
|---------|-----------|-------|
| Real-time edit feed | No | Server-side queue, not real-time feed browsing |
| Edit queue with scoring/prioritization | Yes | Core feature -- centrally stored priority queues based on scoring systems |
| Diff viewing | Yes | Built-in diff browser with PG_UP/PG_DOWN and arrow key scrolling |
| One-click rollback | Yes | "V" key for vandalism revert |
| One-click revert/undo | Yes | Multiple classification options |
| Patrol marking | No | Not a patrol marking tool |
| User warning | Yes | Automated warning posting after revert |
| User talk page integration | Yes | AIV notices to guilty editors |
| ML/AI scoring (ORES/LiftWing) | Indirect | Multiple queue scoring systems: STiki proprietary, ClueBot NG scores, WikiTrust |
| Multi-wiki support | No | English Wikipedia only |
| Coordination between users | Yes | Centrally stored edit queues prevent duplicate review work |
| Offline capability | No | Requires server connection |
| Customizable filters | Partial | Queue selection via "Rev. Queue" menu |
| Whitelist/trusted user management | Partial | Server-side trusted user handling |
| Training data export | Yes | Classification feedback loop improves detection algorithms; training data generated |
| Edit summary customization | Partial | Preset summaries per classification type |
| Watchlist integration | No | Not documented |
| Page protection requests | No | Not a feature |
| Reporting to administrators | Yes | AIV notice generation |
| Block requests | Partial | Through AIV reporting |
| Tag management | No | Not supported |
| Keyboard shortcuts | Yes | V=vandalism, G=good-faith, P=pass, I=innocent (no ALT required) |
| Mobile support | No | Desktop Java application |
| Installable/PWA | N/A | Java JAR executable |
| Dark mode / theming | No | Standard Java Swing UI |

### Distinctive Features
- **Spatio-temporal analysis:** Uses revision metadata (time, location, editor patterns) rather than NLP
- **Multiple scoring queues:** STiki, ClueBot NG, WikiTrust -- user selects which queue to process
- **Centralized deduplication:** Server prevents same edit from being shown to multiple users
- **Four classification actions:** Vandalism (revert+warn), Good-faith (AGF revert), Pass (skip), Innocent (mark OK)
- **Feedback loop:** User classifications improve back-end scoring algorithms

### Potential Issues
- Java JAR distribution -- requires JRE installation
- Last GUI release was 2018
- Server back-end status uncertain

---

## 9. Igloo

**Current Status:** Abandoned (last activity March 2014, 191 commits)
**Platform:** Browser-based userscript (JavaScript)
**Technology:** JavaScript (100%)
**Developer:** User:Kangaroopower
**Source:** [github.com/Kangaroopower/Igloo](https://github.com/Kangaroopower/Igloo)
**Installation:** importScript('Wikipedia:Igloo/gloo.js')

### Features

| Feature | Supported | Notes |
|---------|-----------|-------|
| Real-time edit feed | Yes | Left panel refreshes every 5 seconds with recent edits |
| Edit queue with scoring/prioritization | Partial | Built-in vandalism detection engine |
| Diff viewing | Yes | Click edit in feed to view diff |
| One-click rollback | Yes | Revert vandalism |
| One-click revert/undo | Yes | Revert functionality |
| Patrol marking | No | Not documented |
| User warning | Yes | Warning and reporting/blocking users |
| User talk page integration | Yes | Warning posting |
| ML/AI scoring (ORES/LiftWing) | No | Predates ORES; uses built-in heuristics |
| Multi-wiki support | No | English Wikipedia only |
| Coordination between users | No | Individual tool |
| Offline capability | No | Requires live connection |
| Customizable filters | Yes | Edit filters documented |
| Whitelist/trusted user management | Not documented | |
| Training data export | No | Not supported |
| Edit summary customization | Not documented | |
| Watchlist integration | No | Not documented |
| Page protection requests | No | Not documented |
| Reporting to administrators | Yes | Report or block users |
| Block requests | Yes | Blocking support (for admins) |
| Tag management | No | Not supported |
| Keyboard shortcuts | Not documented | |
| Mobile support | No | Not designed for mobile |
| Installable/PWA | No | Userscript |
| Dark mode / theming | No | No theming |

### Distinctive Features
- Full in-browser GUI (no desktop install needed)
- Profanity highlighting (pink highlight on potentially profane words)
- Was meant as browser-based alternative to desktop tools like Huggle

### Why It Was Abandoned
- Developer (Kangaroopower) stopped contributing around 2014
- Was still in "testing" status when development ceased
- Legacy repository exists at github.com/Kangaroopower/foo
- Not a successor to Huggle -- was an independent parallel effort

---

## 10. ClueBot NG

**Current Status:** Active (v2.0.0, March 2026; operational since 2010)
**Platform:** Automated bot (runs on Toolforge)
**Technology:** PHP (99.3% for bot), C++ (ANN framework), Java (dataset review interface), Python (dataset management), Bash (training scripts)
**Source:** [github.com/cluebotng/bot](https://github.com/cluebotng/bot) (252 commits)
**Hosted:** [cluebotng.toolforge.org](https://cluebotng.toolforge.org)

### Features

| Feature | Supported | Notes |
|---------|-----------|-------|
| Real-time edit feed | Yes | Scans every edit to Wikipedia in real-time |
| Edit queue with scoring/prioritization | Yes | Neural network scoring with Bayesian classifiers |
| Diff viewing | N/A | Automated -- no human diff viewing |
| One-click rollback | N/A | Fully automated revert within 5-30 seconds |
| One-click revert/undo | N/A | Automated |
| Patrol marking | N/A | Automated |
| User warning | Yes | Posts warnings to vandal talk pages |
| User talk page integration | Yes | Automated warning messages |
| ML/AI scoring | Yes | Core feature -- artificial neural network + Bayesian classifiers |
| Multi-wiki support | No | English Wikipedia only |
| Coordination between users | N/A | Automated bot |
| Offline capability | N/A | Server-side |
| Customizable filters | N/A | Not user-configurable |
| Whitelist/trusted user management | Yes | Community whitelist, edit count check, warning ratio check |
| Training data export | Yes | Training dataset available via review interface (cluebotng-review.toolforge.org) |
| Edit summary customization | N/A | Standardized bot edit summaries |
| Watchlist integration | N/A | Automated |
| Page protection requests | No | Not a feature |
| Reporting to administrators | N/A | Self-contained |
| Block requests | No | Does not request blocks |
| Tag management | No | Not supported |
| Keyboard shortcuts | N/A | Automated |
| Mobile support | N/A | Server-side bot |
| Installable/PWA | N/A | Server-side |
| Dark mode / theming | N/A | No UI |

### Distinctive Features
- **Neural network detection:** Learns vandalism patterns from pre-classified dataset (not rule-based)
- **Speed:** Majority of reverts within 5 seconds of vandalism edit
- **False positive rate:** Tuned to 0.25%, catches ~55% of all vandalism
- **1RR safety:** Same user/page combination not reverted more than once per day
- **Angry revert list:** Exception list for pages needing more aggressive monitoring
- **Edit count protection:** Users with many edits and few warnings are not auto-reverted
- **Review interface:** Public dataset review at cluebotng-review.toolforge.org for training data curation

---

## 11. ORES / LiftWing

**Current Status:** ORES deprecated; LiftWing is the active successor
**Platform:** Web service / REST API
**Technology:** ORES: Python (revscoring library); LiftWing: Kubernetes + KServe infrastructure
**ORES Source:** [github.com/wikimedia/ores](https://github.com/wikimedia/ores)

### Migration Timeline
- **Pre-2023:** ORES operational as standalone scoring service
- **September 2023:** ORES API endpoint migrated to LiftWing infrastructure (same API endpoint maintained)
- **January 2025 (tentative):** Revscoring-based models (damaging, goodfaith, reverted) deprecated in favor of Revert Risk models
- **Ongoing:** Models available on LiftWing but no longer improved by ML team

### ORES Models (Legacy, on LiftWing)
- **damaging:** Probability an edit is damaging (0-1 score)
- **goodfaith:** Probability an edit was made in good faith (0-1 score)
- **reverted:** Probability an edit will be reverted
- **articlequality:** Article quality classification
- **draftquality:** Draft quality assessment
- **drafttopic:** Draft topic classification

### LiftWing Models (New/Current)
- **revertrisk-language-agnostic:** Language-agnostic revert risk prediction
- **revertrisk-multilingual:** Multilingual revert risk (LLM-based, ~1s median serving time)
- **articletopic-outlink-transformer:** Article topic classification
- **language-identification:** Language detection
- **logo-detection:** Logo detection
- **readability:** Readability scoring
- **reference-quality:** Reference quality assessment

### Integration Points
- Huggle: ORES scores displayed with configurable thresholds
- RTRC: ORES scoring for feed highlighting
- AntiVandal: ORES prioritization of edit queue
- Recent Changes filters: Built-in MediaWiki integration
- ClueBot NG: Independent ML (does not use ORES)
- SWViewer: Not documented
- Twinkle: Not directly integrated

### Known Issues
- During ORES-to-LiftWing migration, some wikis' RC feeds classified nearly all edits as "very likely bad faith" due to threshold configuration errors
- Revscoring models frozen -- no retraining or code updates
- Multilingual revert risk model has slower serving time (~1s) compared to legacy models

---

## 12. Patroller (MediaWiki Extension)

**Current Status:** Available but minimally maintained
**Platform:** MediaWiki extension (server-side)
**Technology:** PHP (MediaWiki extension framework)
**Requirements:** MediaWiki 1.32+
**Original Author:** Rob Church (2006); adopted by Developaws (2015)

### Features

| Feature | Supported | Notes |
|---------|-----------|-------|
| Real-time edit feed | Yes | Enhanced recent changes interface |
| Edit queue with scoring/prioritization | Partial | Filters incoming edits |
| Diff viewing | Yes | Through MediaWiki diff system |
| One-click rollback | No | Standard MediaWiki rollback |
| One-click revert/undo | No | Standard MediaWiki undo |
| Patrol marking | Yes | Core feature -- prevents self-patrol, shares workload |
| User warning | No | Not a warning tool |
| User talk page integration | No | Not included |
| ML/AI scoring (ORES/LiftWing) | No | Not integrated |
| Multi-wiki support | Yes | Any MediaWiki installation |
| Coordination between users | Yes | Workload sharing between patrollers |
| Offline capability | No | Server-side |
| Customizable filters | Partial | Edit filtering |
| Whitelist/trusted user management | Partial | Self-patrol prevention |
| Training data export | No | Not supported |
| Keyboard shortcuts | No | Not documented |
| Mobile support | Depends on skin | |
| Dark mode / theming | Depends on skin | |

### Other Patrol Gadgets
- **"Mark as patrolled" gadget:** Adds patrol link to Special:RecentChanges, removing need to click (diff)
- **RC Patrol script:** Lightweight script using ORES to auto-determine review priority

---

## 13. WikiPatrol / Wikipedia Android Edit Patrol

### WikiPatrol (Commercial Service)
**Current Status:** Active commercial service (since 2010)
**Platform:** Web service
**Note:** This is NOT a Wikipedia community tool. It is a commercial Wikipedia monitoring service for brands, athletes, politicians, and companies. It provides 24/7 monitoring, instant alerts, and analytics reports for Wikipedia page changes. Not relevant to community patrol tools.

### Wikipedia Android Edit Patrol (Official)
**Current Status:** Active (launched 2024)
**Platform:** Wikipedia Android app (built-in feature)
**Technology:** Kotlin/Java (Android native)
**Developer:** Wikimedia Foundation Android team
**Requirements:** Rollback rights on at least one wiki

### Features

| Feature | Supported | Notes |
|---------|-----------|-------|
| Real-time edit feed | Yes | Recent changes feed within the app |
| Edit queue with scoring/prioritization | Partial | Swipeable edit cards |
| Diff viewing | Yes | In-app diff viewing |
| One-click rollback | Yes | Rollback via toolbar |
| One-click revert/undo | Yes | Undo via toolbar |
| Patrol marking | Partial | Through revert actions |
| User warning | Partial | Talk page messaging |
| User talk page integration | Yes | Personal library of saved talk page messages |
| ML/AI scoring (ORES/LiftWing) | Not documented | |
| Multi-wiki support | Yes | Available on ID, ES, FR, ZH, EN Wikipedias; global rollbackers can patrol all wikis |
| Coordination between users | No | Individual tool |
| Offline capability | No | Requires internet |
| Customizable filters | Limited | Language selection |
| Whitelist/trusted user management | No | Not documented |
| Training data export | No | Not supported |
| Edit summary customization | Partial | Through talk page messages |
| Watchlist integration | Yes | "Watch page" action in toolbar |
| Page protection requests | No | Not available |
| Reporting to administrators | No | Not in v1/v2 |
| Block requests | No | Not available |
| Tag management | No | Not supported |
| Keyboard shortcuts | N/A | Touch-based |
| Mobile support | Yes | Core platform -- native Android app |
| Installable/PWA | Yes | Native app via Google Play |
| Dark mode / theming | Yes | Follows Android system theme |

### Distinctive Features
- **Swipe-to-review:** Swipe through edits for review
- **Editor context:** Shows how long a user has been an editor
- **Template library:** v2 added ability to search, insert, and preview templates
- **Personal message library:** Save and reuse talk page messages
- **Pilot program:** Indonesian Wikipedia served as pilot; expanded to ES, FR, ZH, EN
- **Usage stats (first 30 days):** 63 unique users, 23.8% completed an undo, 12.7% completed a rollback

### No iOS Version
- As of 2026, Edit Patrol is Android-only

---

## 14. Feature Comparison Matrix

### Legend
- **Y** = Yes, fully supported
- **P** = Partial / limited support
- **N** = No / not supported
- **N/A** = Not applicable (e.g., bot has no UI)
- **?** = Unknown / undocumented

| Feature | Huggle | LiveRC | SWViewer | Twinkle | RedWarn | RTRC | AntiVandal | STiki | Igloo | ClueBot NG | Android Patrol |
|---------|--------|--------|----------|---------|---------|------|------------|-------|-------|------------|----------------|
| **Status** | Active | Legacy | Active | Active | Maint. | Active | Active | Semi | Dead | Active | Active |
| **Platform** | Desktop | Gadget | Web app | Gadget | Script | Gadget | Script | Desktop | Script | Bot | Mobile app |
| **Real-time feed** | Y | Y | Y | N | P | Y | Y | N | Y | Y | Y |
| **Edit queue/scoring** | Y | P | Y | N | N | P | Y | Y | P | Y | P |
| **Diff viewing** | Y | Y | Y | Y | Y | Y | Y | Y | Y | N/A | Y |
| **One-click rollback** | Y | Y | Y | Y | Y | N | Y | Y | Y | N/A | Y |
| **One-click revert** | Y | Y | Y | Y | Y | N | Y | Y | Y | N/A | Y |
| **Patrol marking** | Y | Y | P | N | Y | Y | P | N | N | N/A | P |
| **User warning** | Y | Y | Y | Y | Y | N | Y | Y | Y | Y | P |
| **Talk page integration** | Y | Y | Y | Y | Y | N | Y | Y | Y | Y | Y |
| **ORES/LiftWing** | Y | N | N | N | P | Y | Y | P | N | Own ML | ? |
| **Multi-wiki** | Y | P | Y | P | N | Y | N | N | N | N | Y |
| **User coordination** | P | N | P | N | N | Y | N | Y | N | N/A | N |
| **Filters** | Y | Y | Y | N/A | P | Y | Y | P | Y | N/A | P |
| **Whitelist** | Y | P | P | N | N | N | P | P | ? | Y | N |
| **Training export** | N | N | N | N | N | N | N | Y | N | Y | N |
| **Edit summary custom** | Y | P | Y | Y | Y | N/A | Y | P | ? | N/A | P |
| **Watchlist** | P | Y | N | Y | P | N | P | N | N | N/A | Y |
| **Page protection req** | N | N | N | Y | Y | N | N | N | N | N | N |
| **Admin reporting** | Y | P | Y | Y | Y | N | P | Y | Y | N/A | N |
| **Block requests** | P | N | P | Y | Y | N | N | P | Y | N | N |
| **Tag management** | N | N | N | Y | P | N | N | N | N | N | N |
| **Keyboard shortcuts** | Y | P | Y | P | P | P | ? | Y | ? | N/A | N/A |
| **Mobile support** | N | N | Y | P | N | P | ? | N | N | N/A | Y |
| **Dark mode** | N | N | N | N | Planned | N | Y | N | N | N/A | Y |

### Key Observations

1. **Huggle remains the most feature-complete desktop tool** with ORES integration, multi-wiki support, whitelist management, and active development (v3.4.14 in November 2025).

2. **Twinkle is the most widely adopted tool** (~49,000 users) but is action-oriented rather than queue-based -- it augments individual page workflows rather than providing a dedicated patrol feed.

3. **No existing tool combines all desired features.** The gap is particularly notable for:
   - Mobile-first patrol experience (Android Edit Patrol is the only option, and it is limited)
   - Dark mode / theming (only AntiVandal v2.0 and Android app support it)
   - Offline capability (no tool supports this)
   - PWA/installable web app (no tool offers this)
   - Real-time user coordination (only RTRC and STiki offer meaningful deduplication)

4. **ORES/LiftWing migration creates uncertainty.** The transition from revscoring models to Revert Risk models is incomplete, and tools that depend on ORES face potential disruption.

5. **Several tools are abandoned or stagnant:**
   - Igloo: Abandoned 2014
   - STiki: Last GUI release 2018, back-end status uncertain
   - LiveRC: No active development since ~2014
   - RedWarn: Maintenance-only, awaiting Ultraviolet

6. **Browser-based tools are trending** -- AntiVandal v2.0, Ultraviolet (in dev), and Android Edit Patrol represent the modern direction, while desktop tools (Huggle, STiki) are legacy paradigms.

7. **Training data / feedback loops are rare.** Only STiki and ClueBot NG explicitly support training data export and ML feedback loops.

---

## Sources

- [GitHub: Huggle](https://github.com/huggle/huggle3-qt-lx)
- [Wikipedia: Huggle](https://en.wikipedia.org/wiki/Wikipedia:Huggle)
- [MediaWiki: Huggle Keyboard Shortcuts](https://www.mediawiki.org/wiki/Manual:Huggle/Keyboard_shortcuts)
- [Wikipedia: Huggle Whitelist](https://en.wikipedia.org/wiki/Wikipedia:Huggle/Whitelist)
- [French Wikipedia: LiveRC Documentation](https://fr.wikipedia.org/wiki/Wikip%C3%A9dia:LiveRC/Documentation/Pr%C3%A9sentation/en)
- [French Wikipedia: LiveRC 2.0 Specifications](https://fr.wikipedia.org/wiki/Wikip%C3%A9dia:Patrouille_RC/Projet_LiveRC_2.0/Specifications)
- [Meta: SWViewer](https://meta.wikimedia.org/wiki/SWViewer)
- [GitHub: SWViewer](https://github.com/SWViewer/tool-swviewer)
- [MediaWiki: SWViewer Manual](https://www.mediawiki.org/wiki/Manual:SWViewer)
- [Wikipedia: Twinkle](https://en.wikipedia.org/wiki/Wikipedia:Twinkle)
- [Wikipedia: Twinkle Documentation](https://en.wikipedia.org/wiki/Wikipedia:Twinkle/doc)
- [GitHub: Twinkle](https://github.com/wikimedia-gadgets/twinkle)
- [Meta: TwinkleGlobal](https://meta.wikimedia.org/wiki/User:Xiplus/TwinkleGlobal)
- [Wikipedia: RedWarn](https://en.wikipedia.org/wiki/Wikipedia:RedWarn)
- [Wikipedia: RedWarn Features](https://en.wikipedia.org/wiki/Wikipedia:RedWarn/Features)
- [Wikipedia: Ultraviolet](https://en.wikipedia.org/wiki/Wikipedia:Ultraviolet)
- [GitLab: RedWarn/Ultraviolet](https://gitlab.wikimedia.org/repos/10nm/redwarn-web)
- [Meta: RTRC](https://meta.wikimedia.org/wiki/RTRC)
- [GitHub: RTRC](https://github.com/wikimedia/mediawiki-gadgets-RTRC)
- [Wikipedia: AntiVandal](https://en.wikipedia.org/wiki/Wikipedia:AntiVandal)
- [Wikipedia: STiki](https://en.wikipedia.org/wiki/Wikipedia:STiki)
- [GitHub: STiki](https://github.com/westand/STiki)
- [Wikipedia: Igloo](https://en.wikipedia.org/wiki/Wikipedia:Igloo)
- [GitHub: Igloo](https://github.com/Kangaroopower/Igloo)
- [Wikipedia: ClueBot NG](https://en.wikipedia.org/wiki/ClueBot_NG)
- [GitHub: ClueBot NG](https://github.com/cluebotng/bot)
- [Wikitech: ClueBot NG](https://wikitech.wikimedia.org/wiki/Tool:ClueBot_NG)
- [Wikimedia Europe: ClueBot NG](https://wikimedia.brussels/meet-cluebot-ng-an-anti-vandal-ai-bot-that-tries-to-detect-and-revert-vandalism/)
- [MediaWiki: ORES](https://www.mediawiki.org/wiki/ORES)
- [Wikitech: LiftWing](https://wikitech.wikimedia.org/wiki/Machine_Learning/LiftWing)
- [Wikimedia API: Lift Wing](https://api.wikimedia.org/wiki/Lift_Wing_API)
- [MediaWiki: Patroller Extension](https://www.mediawiki.org/wiki/Extension:Patroller)
- [Wikimedia Diff: Edit Patrol Mobile](https://diff.wikimedia.org/2024/07/10/%D9%90addressing-vandalism-with-a-tap-the-journey-of-introducing-the-patrolling-feature-in-the-mobile-app/)
- [MediaWiki: Android Anti-Vandalism](https://www.mediawiki.org/wiki/Wikimedia_Apps/Team/Android/Anti_Vandalism)
- [Wikipedia: Cleaning Up Vandalism/Tools](https://en.wikipedia.org/wiki/Wikipedia:Cleaning_up_vandalism/Tools)
