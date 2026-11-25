# SPOIberia (`spoiberia`)

[🇪🇸 Español](README.md) | 🇬🇧 English

Cross-platform **Rust** application with a **dual architecture** (**interactive TUI** + **headless CLI for autonomous agents**) for managing permissions, provisioning document libraries and folders from external matrices, and cloning full **SharePoint Online (Microsoft 365)** sites with transactional persistence and **SQLite Rollback**.

---

## Key Features

### Phase 1 — Group & User Permissions + Staging Queue + SQLite Rollback
- **Native OAuth2 Authentication against Microsoft Entra ID**:
  - Supports **Interactive Browser (PKCE)** and **Device Code Flow** with no external binaries required.
  - Automatic multi-resource `refresh_token` exchange for **Microsoft Graph** and **SharePoint Online REST API**.
- **Hybrid SharePoint Site Discovery (Admin / Owner)**:
  - Combines M365 Group owned sites (`/me/ownedObjects`) with SharePoint Search (`contentclass:STS_Site`) and concurrent permission checks (`CurrentUser/IsSiteAdmin` and `EffectiveBasePermissions` bit `ManagePermissions`).
- **Staging Queue & Transactional SQLite Persistence (`rusqlite` bundled)**:
  - In the TUI, additions (`[+]` in green) and removals (`[-]` in red) are queued in **Staging** and only executed when you press **`A` (Apply)**.
  - Each applied batch records a **Transaction** in SQLite (`spoiberia.db`) along with the exact `reverse_payload` for each operation, enabling deterministic **LIFO Rollback** of full transactions or individual operations.

### Phase 2 — Libraries, Folders, Unique Permissions & External Matrix (CSV / JSON)
- Browse and create **Document Libraries** and **Folders/Subfolders** (in *"Documents"* / `Shared Documents` or custom libraries).
- Break permission inheritance (`BreakRoleInheritance`), **auto-create missing SharePoint groups**, and grant language-independent permission levels (`Read`, `Contribute`, `Edit`, `Full Control`).
- **External Matrix Import & Export (`.csv` and `.json`)**:
  - Automatically sorts folders *top-down* by path depth so parent folders are configured before subfolders.

### Phase 3 — Full SharePoint Site Cloning
- End-to-end copy from a **Source** site to a **Target** site within the same tenant, with mode selection:
  - **Additive Mode (`--mode additive`, default)**: keeps existing target content and groups, adding/overwriting items from source.
  - **Destructive Mode (`--mode destructive`, with prominent warnings & double confirmation)**: deletes existing non-default libraries, root folders/files in the default library, custom groups missing from source, and current group members (protecting the current operator in Owners) before cloning.
  1. Synchronizes members in associated groups (`Owners`, `Members`, `Visitors`) and creates custom groups with their members and site roles.
  2. Creates missing document libraries and performs **Server-Side Copy (`SP.MoveCopyUtil`)** of all folders and files while preserving version history without downloading data locally.
  3. Replicates *top-down* **unique folder permissions (broken inheritance)** to the equivalent target groups and users.

### Phase 4 — SharePoint Offline Sync & OneDrive Shortcuts Control
- Managed as two independent suboptions within the same TUI menu (`o` / `O`) and CLI (`spoiberia sync`):
  1. **SharePoint Sync (`sharepoint`)**: toggles `ExcludeFromOfflineClient` via SharePoint Online REST API at both **Site (`SP.Web`)** and **Document Library (`SP.List`)** levels (with recursive subsite support via `--include-subsites`) plus `sync` / `syncCommand` keys in `CustomFormatter`.
  2. **OneDrive Shortcuts (`onedrive`)**: hides or restores only the *"Add shortcut to OneDrive"* keys (`addShortcut`, `addShortcutToOneDriveCommand`) via **JSON View Formatting (`commandBarProps`)** on public library views (`CustomFormatter` on `SP.View`) without affecting the rest of the tenant or blocking file downloads.
- Records previous flags and exact `CustomFormatter` backups in SQLite for full **LIFO Rollback**.

---

## Interactive Usage (TUI)

Run `spoiberia` without arguments (or `spoiberia tui`):

```bash
spoiberia
```

### TUI Language (`--lang`)
- **Auto-detection (`--lang auto`, default)**: detects the OS language on **Linux** (`LC_ALL`/`LC_MESSAGES`/`LANG`), **Windows** (`GetUserDefaultLocaleName`), and **macOS** (`LC_ALL`/`LANG` or `defaults read -g AppleLanguages`). Spanish locales select **Spanish**; all other locales select **English**.
- **Explicit override via CLI flag or environment variable**:
  ```bash
  spoiberia --lang en
  spoiberia --lang es
  SPOIBERIA_LANG=en spoiberia
  ```

### Main TUI Keyboard Shortcuts
- **Global**: `q` (Quit), `?` (Help), `A` (**Apply Staging queue**), `1`/`2`/`3`/`4` (Switch site tabs), `O` (Open SharePoint Sync / OneDrive Shortcuts menu for site + all libraries).
- **Site Selector**: `↑`/`↓` (Navigate), `Enter` (Open site), `/` (Local filter), `s` (Search tenant), `u` (Open manual URL), `c` (Set as Clone Source).
- **Tab 1 (Groups & Users)**: `Tab`/`←`/`→` (Switch pane), `a` (Search M365 user & queue add), `d` (Remove user), `g` (Create group).
- **Tab 2 (Libraries & Folders)**: `o` (Open SharePoint Sync / OneDrive Shortcuts menu for selected library), `O` (Site + all libraries), `n` (New folder), `b` (New library), `p` (Grant unique folder permission), `i` (Import CSV/JSON matrix), `e` (Export CSV/JSON matrix).
- **Tab 3 (Clone Site)**: `Space` (Toggle options, including Additive vs Destructive mode), `s` (Set Source URL), `Enter` (Generate Clone Plan in Staging).
- **Tab 4 (Staging & SQLite History)**: `A` (Apply Staging queue), `R` (**Revert full transaction**), `r` (**Revert single operation**), `d` (Remove item from Staging), `X` (Clear Staging).

---

## Command-Line Mode (CLI for Autonomous Agents & Scripts)

All subcommands support `--json` (or `-o json|table|csv`) and `--dry-run`:

```bash
# 1. Authentication
spoiberia auth status --json
spoiberia auth login [--device-code]

# 2. List or check sites where you are Admin or Owner
spoiberia sites list --json
spoiberia sites check --site "https://contoso.sharepoint.com/sites/MySite" --json

# 3. Groups & members
spoiberia groups list --site "https://contoso.sharepoint.com/sites/MySite" --json
spoiberia groups members --site "https://contoso.sharepoint.com/sites/MySite" --group "MySite Members" --json
spoiberia users search --query "novella" --json

# 4. Add or remove users (recording a reversible SQLite transaction)
spoiberia members add \
  --site "https://contoso.sharepoint.com/sites/MySite" \
  --group "MySite Members" \
  --users "user1@contoso.com,user2@contoso.com" \
  --note "Requested by agent" \
  --dry-run --json

# 5. SQLite Transaction History & Rollback
spoiberia tx list --json
spoiberia tx show 1 --json
spoiberia tx revert 1 --json

# 6. Phase 2: Libraries, Folders & External Matrix (CSV / JSON)
spoiberia folders grant \
  --site "https://contoso.sharepoint.com/sites/MySite" \
  --library "Documents" \
  --path "01_Exec/Confidential" \
  --group "SPO_Exec_Committee" \
  --role "Full Control" \
  --create-group \
  --dry-run --json

spoiberia matrix apply \
  --site "https://contoso.sharepoint.com/sites/MySite" \
  --file ./examples/sample-permissions.csv \
  --dry-run --json

spoiberia matrix export \
  --site "https://contoso.sharepoint.com/sites/MySite" \
  --library "Documents" \
  --out ./exported-permissions.json

# 7. Phase 3: Full Site Cloning (Additive by default, or Destructive)
spoiberia clone \
  --source-site "https://contoso.sharepoint.com/sites/TemplateSite" \
  --target-site "https://contoso.sharepoint.com/sites/NewSite" \
  --dry-run --json

# Destructive mode (deletes existing target content, groups and users; requires --confirm-destructive to execute)
spoiberia clone \
  --source-site "https://contoso.sharepoint.com/sites/TemplateSite" \
  --target-site "https://contoso.sharepoint.com/sites/NewSite" \
  --mode destructive \
  --dry-run --json

# 8. Phase 4: SharePoint Offline Sync & OneDrive Shortcuts Control
spoiberia sync status \
  --site "https://contoso.sharepoint.com/sites/MySite" \
  --include-subsites --json

# 8a. Suboption: SharePoint offline sync only (`ExcludeFromOfflineClient`)
spoiberia sync sharepoint disable \
  --site "https://contoso.sharepoint.com/sites/MySite" \
  --include-subsites \
  --note "Block SharePoint sync" \
  --dry-run --json

# 8b. Suboption: "Add shortcut to OneDrive" button only (`CustomFormatter`)
spoiberia sync onedrive disable \
  --site "https://contoso.sharepoint.com/sites/MySite" \
  --library "Documents" \
  --note "Hide OneDrive shortcut" \
  --dry-run --json

# 8c. Both or via `--target sharepoint|onedrive|all`
spoiberia sync disable \
  --site "https://contoso.sharepoint.com/sites/MySite" \
  --target all \
  --include-subsites \
  --dry-run --json

spoiberia sync enable \
  --site "https://contoso.sharepoint.com/sites/MySite" \
  --library "Documents" \
  --target sharepoint \
  --dry-run --json
```

---

## Multi-Platform Release Build (Linux, Windows & macOS)

Run the build script to generate standalone executables in `./dist/`:

```bash
./scripts/build-release.sh
```

Generated artifacts in `dist/`:
- `dist/spoiberia-linux-x86_64` (Linux ELF 64-bit)
- `dist/spoiberia-windows-x86_64.exe` (Windows PE32+ 64-bit)
- `dist/spoiberia-macos-aarch64` (macOS Apple Silicon Mach-O 64-bit)
