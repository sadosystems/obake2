# Spare parts

Obake crates and components that are unhooked, or slated to be unhooked, from the
obake app (not initialized, not built as part of `obake`) but kept in the tree because
they look reusable. Entries marked "pending" are still hooked in.
Add an entry whenever something useful gets unhooked, so it isn't forgotten.

Format: crate (or file) — what it is — possible obake use — how it was unhooked.

- **search** — buffer and project search bars, query options, match navigation —
  searching traces and logs — init, pane search-bar callbacks and toolbar items removed
- **outline** / **outline_panel** — fuzzy picker and dockable list over a document's
  symbols — jump to a frame or an event — init and panel load removed
- **file_finder** — fuzzy file picker with match highlighting — open recordings or
  cases by name — init removed
- **go_to_line** — small jump-to-position modal — jump to frame N — init removed
- **project_symbols** / **lsp_locations** / **call_hierarchy** — searchable result lists
  — event/trace navigation UI — init removed
- **tasks_ui** — picker plus runner for configured commands — launching obake runs —
  init removed
- **git_ui** — panels, diff views, commit views, blame — diff UI for comparing two
  runs' traces — init, panel load and toolbar items removed
- **debugger_ui** — dockable debug panel: stack, variables, breakpoints, console —
  model for a time-travel debugger pane — init and panel load removed
- **diagnostics** — grouped problem list with jump-to — divergence report list —
  init and toolbar controls removed
- **markdown_preview** / **svg_preview** / **tabular_data_preview** / **image_viewer** —
  preview panes — viewing reports, tables, frame images — init removed
- **quick_action_bar** (`obake/src/obake/quick_action_bar*`) — per-pane toolbar of icon
  actions and menus — obake per-pane controls — no longer added to pane toolbars
- **breadcrumbs** — path/location strip in the pane toolbar — run > frame > event
  location strip — no longer added to pane toolbars

Unhooked with no obvious reuse: vim, journal, repl, snippets_ui, snippet_provider,
edit_prediction(+ui, registry), extensions_ui, dap_adapters, debugger_tools,
language_tools, encoding/language/line-ending/lsp-command/toolchain selectors.
