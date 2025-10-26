# NoteVim - Minimal Markdown Note-Taking in Neovim

A clean, minimal Neovim configuration optimized for markdown note-taking with image support.

## Features

- 📝 **Markdown rendering** with beautiful syntax highlighting
- 🖼️ **Image support** - inline rendering and clipboard paste
- 🎨 **Kanagawa theme** with modern UI via Snacks.nvim
- 📊 **Relative line numbers** in markdown files
- ✨ **Treesitter** syntax highlighting

## Structure

```
~/.config/notevim/
├── init.lua              # Main config with settings
├── lua/
│   ├── notetaking_core.lua  # Markdown & image plugins
│   └── plugins.lua          # UI/prettiness (Snacks, theme)
└── ~/notes/              # Your notes workspace
```

## Usage

### Starting Neovim
```bash
nvim
```

### Taking Notes

1. **Create a note**: Navigate to `~/notes/` and create a `.md` file
   ```bash
   cd ~/notes
   nvim my-note.md
   ```

2. **Paste images**: Copy an image to clipboard, then:
   - Press `<leader>p` (Space + p) in normal mode
   - Image will be saved to `images/` folder
   - Markdown link is automatically inserted
   - Images render inline in Kitty terminal

3. **Markdown features**:
   - Write standard markdown
   - Beautiful rendering of headers, lists, code blocks
   - Syntax highlighting with treesitter

4. **Create tables easily**:
   - Press `<leader>tm` to toggle table mode on
   - Type `||` and press Enter to start a table
   - Tables auto-format and align as you type!
   - Type `||` again on header row to create separator
   - **Navigate cells**: `]|` (next), `[|` (prev), `}|` (down), `{|` (up)
   - Table mode keeps columns aligned automatically

### Linking Notes

- **Press `Enter`** on a link to follow it (mkdnflow)
- Standard markdown links work: `[note](path.md)`
- `gf` to open file under cursor

### Telekasten (No external dependencies!)

- `<leader>zn` - Create new note
- `<leader>zo` - Show calendar/daily notes
- `<leader>zf` - Find notes by filename
- `<leader>zt` - Find notes by tags
- `<leader>zg` - Search content in notes
- `<leader>zh` - Follow link under cursor
- `<leader>zb` - Show backlinks to current note
- `<leader>zl` - Insert link to another note

### Key Bindings

**Tip:** Press `Space` and wait - which-key will show you all available commands!

**File Navigation:**
| Key | Action |
|-----|--------|
| `<leader><leader>` | Quick find files (Space + Space) |
| `<leader>e` | Toggle file tree |
| `<leader>ff` | Find files (telescope) |
| `<leader>fg` | Live grep search |
| `<leader>fb` | List buffers |
| `<leader>fh` | Search help |
| `gf` | Open file under cursor |

**Window Navigation:**
| Key | Action |
|-----|--------|
| `Ctrl+h` | Move to left window |
| `Ctrl+j` | Move to window below |
| `Ctrl+k` | Move to window above |
| `Ctrl+l` | Move to right window |

**Note Taking:**
| Key | Action |
|-----|--------|
| `<leader>p` | Paste image from clipboard |
| `<leader>tm` | Toggle table mode |
| Type `\|\|` in table mode | Auto-creates table |
| When typing `-` or `*` | Auto-continues list on Enter |
| `Tab` in insert | Indent current line/list item |
| `Shift+Tab` in insert | Unindent current line/list item |
| `>` in visual | Indent selection |
| `<` in visual | Unindent selection |

**Git & UI:**
| Key | Action |
|-----|--------|
| `<leader>n` | Show notification history |
| `<leader>bd` | Delete buffer |
| `<leader>gg` | Open Lazygit |
| `<leader>gb` | Git blame current line |

## Requirements

- Neovim 0.9+
- **Kitty terminal** (for inline image rendering)
- Git (for plugin management)
- xclip or wl-clipboard (for image pasting from clipboard)
- lua5.1 and luarocks (for image.nvim) - see installation notes below

## NixOS Installation

To install lua5.1 and luarocks on NixOS:

### Option 1: Temporary shell (quick test)
```bash
nix-shell -p lua5_1 luarocks
```

### Option 2: Add to your environment (persistent)
Add to your `~/.config/nixpkgs/home.nix` (if using home-manager):
```nix
home.packages = with pkgs; [
  lua5_1
  luarocks
];
```

Or add to your system configuration `/etc/nixos/configuration.nix`:
```nix
environment.systemPackages = with pkgs; [
  lua5_1
  luarocks
];
```

Then rebuild:
```bash
sudo nixos-rebuild switch
```

## Notes Directory

Your notes live in `~/notes/`. Start writing markdown files there!

