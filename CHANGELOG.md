# Changelog

All notable changes to Shovel are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Responsiveness architecture** — design spec and implementation plan to split
  the monolithic tab-state signal into four independent signals, rewrite the JS
  bridge, move heavy work off the render thread, and add memoization boundaries.
- **AI Query Optimizer** — design spec and implementation plan for an
  ACP-assisted query optimization workflow.
- **Full UI description** (`docs/ui-description.md`) — a single source of truth
  for how the interface is structured, for contributors and agents.
- **GitHub community files** — Dependabot config, issue templates (bug, feature,
  documentation), and a pull-request template.
- **Corrected packaging metadata** — MIT license now declared consistently across
  the Flatpak metainfo and all Arch/AUR `PKGBUILD` files.

### Changed

- **Explorer keyboard workflows** — F2 renames and Delete drops the selected
  table directly from the tree, mirroring the context-menu actions.
- **Explorer polish** — single-click table open, restrained explorer/toolbar,
  agent-panel thinking animation, shortcut registry, and grid cell navigation.
- **Compact layout for small screens** — smaller editor/dock defaults,
  density-aware toolbar and status bar, tighter tabs, responsive breakpoints,
  and context-menu styling.
- **Workspace split modes** — the SQL editor and results can now be laid out
  Off / Horizontal / Vertical, persisted in settings.
- **Bottom dock** — a resizable, hideable dock with Output / Messages / Query
  Log / Transactions / Problems tabs, persisted in settings.
- **Settings as a category-navigated editor** — Appearance / Database / Editor /
  Grid / Navigation / Advanced categories, reusable in the in-app overlay and a
  standalone native window.
- **Mock database repository** — a dev-only fake session for UI work without a
  running database (debug builds only).
- **Hard read-only gate for the agent SQL tool** — the ACP agent's SQL execution
  respects read-only mode.

### Fixed

- Pre-existing clippy lints across the UI crate (collapsible `if`, `useless_vec`,
  `map_identity`, field reassignment, `too_many_arguments`, redundant closures).

## [0.1.0] - Initial release

### Added

- SQLite, PostgreSQL, MySQL, and ClickHouse support
- Schema explorer with table management
- SQL editor with syntax highlighting and multi-tab support
- Query execution with paginated results, export, and editing
- ER diagram viewer
- Execution plan visualization
- Data diff tool
- AI-powered chat via ACP agents (Codex-ACP, OpenCode, embedded DeepSeek)
- Saved queries and query history
- Dark/light theming
- SSH tunnel support
- CSV/JSON/XLSX/XML/HTML/SQL export
- Flatpak, APT, Arch Linux, AUR, and Windows packaging
