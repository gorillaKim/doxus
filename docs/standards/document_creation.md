# Doxus Document Creation Standard

This document outlines the standard procedure for document creation across all Doxus plugins (Confluence, GitHub, Obsidian, etc.).

## 1. Hierarchical Path Parsing

All plugins must use the `doxus_plugin_sdk::path_utils::parse_hierarchical_path` utility to ensure consistent handling of nested folders and document titles.

-   **Input**: A folder path string (optional) and a title string.
-   **Output**: A vector of path segments.
-   **Security**: The utility automatically detects and prevents path traversal attacks (`..`).

## 2. Conflict Resolution (Option B Policy)

Doxus adopts the **Automatic Numeric Suffixing** policy (Option B) for all write operations.

-   If a document with the same title already exists in the target location, the plugin should **not** return an error.
-   Instead, it should append a numeric suffix in the format ` (N)`.
-   Example: `My Document` -> `My Document (1)` -> `My Document (2)`.
-   The implementation should attempt this recursively (up to 10 attempts recommended).

## 3. Confluence-Specific Hierarchy

For Confluence, the path corresponds to the parent-child page hierarchy within a Space.
-   The "Folder" input matches the ancestor page titles.
-   If intermediate pages do not exist, they should be created automatically as placeholder pages.

## 4. Error Handling (PDK)

All WASM plugins must correctly map internal errors to `PluginError`.
-   When returning errors from `extism_pdk` functions, always use the `.0.to_string()` method to extract the error message from the result tuple.

---
*Last Updated: 2026-04-16*
