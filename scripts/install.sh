#!/bin/bash

# EDB - Ethereum Debugger
# Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU Affero General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
# GNU Affero General Public License for more details.
#
# You should have received a copy of the GNU Affero General Public License
# along with this program. If not, see <https://www.gnu.org/licenses/>.
#
# This script installs EDB (tries pre-built binaries first, falls back to source)
# Run with: curl -sSL https://install.edb.sh | bash

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Print colored messages
print_error() {
    echo -e "${RED}Error: $1${NC}" >&2
}

print_success() {
    echo -e "${GREEN}$1${NC}"
}

print_info() {
    echo -e "${YELLOW}$1${NC}"
}

# Detect OS
detect_os() {
    case "$(uname -s)" in
        Linux*)     echo "linux";;
        Darwin*)    echo "macos";;
        CYGWIN*)    echo "windows";;
        MINGW*)     echo "windows";;
        *)          echo "unknown";;
    esac
}

# Detect architecture
detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)   echo "x86_64";;
        aarch64|arm64)  echo "aarch64";;
        *)              echo "unknown";;
    esac
}

OS=$(detect_os)
ARCH=$(detect_arch)
print_info "Detected OS: $OS"
print_info "Detected Architecture: $ARCH"

# Try to download and install pre-built binaries
try_binary_install() {
    # Check if platform is supported for binary releases
    if [ "$OS" = "unknown" ] || [ "$ARCH" = "unknown" ]; then
        print_info "Pre-built binaries not available for $OS-$ARCH"
        return 1
    fi

    # Construct the binary name
    local BINARY_NAME="edb-${OS}-${ARCH}"
    local EXTENSION=".tar.gz"
    if [ "$OS" = "windows" ]; then
        EXTENSION=".zip"
    fi

    local DOWNLOAD_URL="https://github.com/edb-rs/edb/releases/latest/download/${BINARY_NAME}${EXTENSION}"

    print_info "Attempting to download pre-built binaries from GitHub releases..."
    print_info "URL: $DOWNLOAD_URL"

    # Create temporary directory
    local TEMP_DIR=$(mktemp -d)
    trap "rm -rf $TEMP_DIR" EXIT

    # Download the binary archive (keep original filename for checksum verification)
    local ARCHIVE_FILE="${BINARY_NAME}${EXTENSION}"

    if command -v curl &> /dev/null; then
        if ! curl -L --fail -o "$TEMP_DIR/$ARCHIVE_FILE" "$DOWNLOAD_URL" 2>/dev/null; then
            print_info "Failed to download pre-built binaries"
            return 1
        fi
        # Download checksum file
        if ! curl -L --fail -o "$TEMP_DIR/${ARCHIVE_FILE}.sha256" "${DOWNLOAD_URL}.sha256" 2>/dev/null; then
            print_info "Failed to download checksum file"
            return 1
        fi
    elif command -v wget &> /dev/null; then
        if ! wget -q -O "$TEMP_DIR/$ARCHIVE_FILE" "$DOWNLOAD_URL" 2>/dev/null; then
            print_info "Failed to download pre-built binaries"
            return 1
        fi
        # Download checksum file
        if ! wget -q -O "$TEMP_DIR/${ARCHIVE_FILE}.sha256" "${DOWNLOAD_URL}.sha256" 2>/dev/null; then
            print_info "Failed to download checksum file"
            return 1
        fi
    else
        print_info "Neither curl nor wget available for downloading"
        return 1
    fi

    print_success "✓ Downloaded pre-built binaries"

    # Verify checksum
    print_info "Verifying checksum..."

    # Change to temp dir and verify (using subshell to avoid cd issues)
    if command -v shasum &> /dev/null; then
        if ! (cd "$TEMP_DIR" && shasum -a 256 -c "${ARCHIVE_FILE}.sha256") 2>/dev/null; then
            print_error "Checksum verification failed!"
            print_error "The downloaded file may be corrupted or tampered with."
            return 1
        fi
    elif command -v sha256sum &> /dev/null; then
        if ! (cd "$TEMP_DIR" && sha256sum -c "${ARCHIVE_FILE}.sha256") 2>/dev/null; then
            print_error "Checksum verification failed!"
            print_error "The downloaded file may be corrupted or tampered with."
            return 1
        fi
    else
        print_info "⚠️  Warning: No SHA256 tool available (shasum/sha256sum)"
        print_info "⚠️  Skipping checksum verification"
        echo ""
    fi

    print_success "✓ Checksum verified"

    # Extract the archive
    print_info "Extracting binaries..."
    if [ "$OS" = "windows" ]; then
        if ! unzip -q "$TEMP_DIR/$ARCHIVE_FILE" -d "$TEMP_DIR" 2>/dev/null; then
            print_error "Failed to extract archive"
            return 1
        fi
    else
        if ! tar xzf "$TEMP_DIR/$ARCHIVE_FILE" -C "$TEMP_DIR" 2>/dev/null; then
            print_error "Failed to extract archive"
            return 1
        fi
    fi

    print_success "✓ Extracted binaries"

    # Create cargo bin directory if it doesn't exist
    local CARGO_BIN="$HOME/.cargo/bin"
    mkdir -p "$CARGO_BIN"

    # Install binaries
    print_info "Installing binaries to $CARGO_BIN..."

    local EXT=""
    if [ "$OS" = "windows" ]; then
        EXT=".exe"
    fi

    if [ -f "$TEMP_DIR/edb${EXT}" ]; then
        cp "$TEMP_DIR/edb${EXT}" "$CARGO_BIN/"
        chmod +x "$CARGO_BIN/edb${EXT}"
    else
        print_error "Binary edb${EXT} not found in archive"
        return 1
    fi

    if [ -f "$TEMP_DIR/edb-rpc-proxy${EXT}" ]; then
        cp "$TEMP_DIR/edb-rpc-proxy${EXT}" "$CARGO_BIN/"
        chmod +x "$CARGO_BIN/edb-rpc-proxy${EXT}"
    fi

    if [ -f "$TEMP_DIR/edb-tui${EXT}" ]; then
        cp "$TEMP_DIR/edb-tui${EXT}" "$CARGO_BIN/"
        chmod +x "$CARGO_BIN/edb-tui${EXT}"
    fi

    print_success "✓ Installed binaries to $CARGO_BIN"
    return 0
}

# Check if git is installed
check_git() {
    if ! command -v git &> /dev/null; then
        print_error "git is not installed"
        echo ""
        echo "Please install git first:"
        case "$OS" in
            linux)
                echo "  Ubuntu/Debian: sudo apt-get install git"
                echo "  Fedora/RHEL:   sudo dnf install git"
                echo "  Arch:          sudo pacman -S git"
                ;;
            macos)
                echo "  Using Homebrew: brew install git"
                echo "  Or download from: https://git-scm.com/download/mac"
                ;;
            windows)
                echo "  Download from: https://git-scm.com/download/win"
                ;;
            *)
                echo "  Visit: https://git-scm.com/downloads"
                ;;
        esac
        exit 1
    fi
    print_success "✓ git is installed"
}

# Check if cargo is installed
check_cargo() {
    if ! command -v cargo &> /dev/null; then
        print_error "cargo (Rust toolchain) is not installed"
        echo ""
        echo "Please install Rust and Cargo first:"
        case "$OS" in
            linux|macos)
                echo "  Run: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
                ;;
            windows)
                echo "  Download from: https://rustup.rs/"
                ;;
            *)
                echo "  Visit: https://rustup.rs/"
                ;;
        esac
        echo ""
        echo "After installation, restart your terminal and run this script again."
        exit 1
    fi
    print_success "✓ cargo is installed"
}

# Check if bun is installed (required by crates/web/build.rs to bundle the web UI)
check_bun() {
    if ! command -v bun &> /dev/null; then
        print_error "bun is not installed"
        echo ""
        echo "EDB's web UI is bundled at compile time and requires bun on PATH."
        echo "Please install bun first:"
        case "$OS" in
            linux|macos)
                echo "  Run: curl -fsSL https://bun.sh/install | bash"
                echo "  Then restart your shell (or 'source ~/.bashrc' / 'source ~/.zshrc')"
                echo "  so 'bun --version' resolves before re-running this script."
                ;;
            windows)
                echo "  Run in PowerShell: irm bun.sh/install.ps1 | iex"
                echo "  Or visit: https://bun.sh"
                ;;
            *)
                echo "  Visit: https://bun.sh"
                ;;
        esac
        echo ""
        echo "Alternatively, if you don't need the web UI, you can bypass the bundle"
        echo "by setting EDB_SKIP_WEB_BUILD=1 and running the source build manually:"
        echo "  EDB_SKIP_WEB_BUILD=1 cargo install --path crates/edb"
        echo "  EDB_SKIP_WEB_BUILD=1 cargo install --path crates/rpc-proxy"
        echo "  EDB_SKIP_WEB_BUILD=1 cargo install --path crates/tui"
        echo "(The TUI and CLI work; 'edb replay --ui=web' will fail at runtime"
        echo " until you rebuild with bun installed.)"
        exit 1
    fi
    print_success "✓ bun is installed"
}

# Create ~/.edb directory if it doesn't exist
create_edb_dir() {
    EDB_DIR="$HOME/.edb"
    if [ ! -d "$EDB_DIR" ]; then
        print_info "Creating directory: $EDB_DIR"
        mkdir -p "$EDB_DIR"
        print_success "✓ Created $EDB_DIR"
    else
        print_success "✓ Directory $EDB_DIR already exists"
    fi
}

# Clone the repository
clone_repo() {
    EDB_DIR="$HOME/.edb"
    REPO_DIR="$EDB_DIR/edb"

    if [ -d "$REPO_DIR" ]; then
        print_info "Repository already exists at $REPO_DIR"

        # Check if running in interactive mode
        if [ -t 0 ]; then
            read -p "Do you want to pull the latest changes? (y/n) " -n 1 -r
            echo
            if [[ $REPLY =~ ^[Yy]$ ]]; then
                print_info "Pulling latest changes..."
                cd "$REPO_DIR"
                if ! git pull; then
                    print_error "Failed to pull latest changes"
                    exit 1
                fi
                print_success "✓ Updated repository"
            fi
        else
            # Non-interactive mode (piped from curl), auto-pull
            print_info "Pulling latest changes (non-interactive mode)..."
            cd "$REPO_DIR"
            if ! git pull; then
                print_error "Failed to pull latest changes"
                exit 1
            fi
            print_success "✓ Updated repository"
        fi
    else
        print_info "Cloning repository to $REPO_DIR..."
        if ! git clone https://github.com/edb-rs/edb "$REPO_DIR"; then
            print_error "Failed to clone repository"
            exit 1
        fi
        print_success "✓ Cloned repository"
    fi
}

# Ensure cargo bin directory is in PATH
ensure_cargo_path() {
    CARGO_BIN="$HOME/.cargo/bin"

    if [[ ":$PATH:" != *":$CARGO_BIN:"* ]]; then
        echo ""
        print_info "⚠️  Warning: $CARGO_BIN is not in your PATH"
        echo ""
        echo "To use EDB commands, you need to add it to your PATH."
        echo "Add the following line to your shell configuration file:"
        echo ""
        case "$OS" in
            linux)
                if [ -f "$HOME/.bashrc" ]; then
                    echo "  echo 'export PATH=\"\$HOME/.cargo/bin:\$PATH\"' >> ~/.bashrc"
                    echo "  source ~/.bashrc"
                elif [ -f "$HOME/.zshrc" ]; then
                    echo "  echo 'export PATH=\"\$HOME/.cargo/bin:\$PATH\"' >> ~/.zshrc"
                    echo "  source ~/.zshrc"
                else
                    echo "  export PATH=\"\$HOME/.cargo/bin:\$PATH\""
                fi
                ;;
            macos)
                if [ -f "$HOME/.zshrc" ]; then
                    echo "  echo 'export PATH=\"\$HOME/.cargo/bin:\$PATH\"' >> ~/.zshrc"
                    echo "  source ~/.zshrc"
                elif [ -f "$HOME/.bash_profile" ]; then
                    echo "  echo 'export PATH=\"\$HOME/.cargo/bin:\$PATH\"' >> ~/.bash_profile"
                    echo "  source ~/.bash_profile"
                else
                    echo "  export PATH=\"\$HOME/.cargo/bin:\$PATH\""
                fi
                ;;
            *)
                echo "  export PATH=\"\$HOME/.cargo/bin:\$PATH\""
                ;;
        esac
        echo ""
        echo "Or restart your terminal after installation."
        echo ""
    else
        print_success "✓ $CARGO_BIN is in your PATH"
    fi
}

# Install EDB components
install_edb() {
    REPO_DIR="$HOME/.edb/edb"
    cd "$REPO_DIR"

    print_info "Building and installing EDB components..."
    print_info "This may take a few minutes..."
    echo ""

    # Install main edb binary
    print_info "Installing edb..."
    if ! cargo install --path crates/edb; then
        print_error "Failed to install edb"
        exit 1
    fi
    print_success "✓ Installed edb"

    # Install rpc-proxy
    print_info "Installing edb-rpc-proxy..."
    if ! cargo install --path crates/rpc-proxy; then
        print_error "Failed to install edb-rpc-proxy"
        exit 1
    fi
    print_success "✓ Installed edb-rpc-proxy"

    # Install tui
    print_info "Installing edb-tui..."
    if ! cargo install --path crates/tui; then
        print_error "Failed to install edb-tui"
        exit 1
    fi
    print_success "✓ Installed edb-tui"
}

# Main installation flow
main() {
    echo ""
    print_info "=========================================="
    print_info "  EDB Installation Script"
    print_info "=========================================="
    echo ""

    # Try binary installation first
    print_info "Step 1/2: Attempting to install pre-built binaries..."
    echo ""

    if try_binary_install; then
        print_success "✓ Successfully installed pre-built binaries"
        echo ""

        # Ensure PATH includes cargo bin
        ensure_cargo_path

        # Success message
        echo ""
        print_success "=========================================="
        print_success "  EDB Installation Complete!"
        print_success "=========================================="
        echo ""
        print_info "You can now use EDB by running:"
        echo "  edb --help"
        echo ""
        print_info "To debug a transaction:"
        echo "  edb --rpc-urls <RPC_ENDPOINT> replay <TX_HASH>"
        echo ""
        print_info "For more information, visit:"
        echo "  https://github.com/edb-rs/edb"
        echo ""
        return 0
    fi

    # Fallback to source installation
    echo ""
    print_info "=========================================="
    print_info "  Pre-built binaries not available"
    print_info "=========================================="
    echo ""

    print_info "Pre-built binaries are available for:"
    echo "  • Linux:   x86_64, aarch64"
    echo "  • macOS:   x86_64 (Intel), aarch64 (Apple Silicon)"
    echo "  • Windows: x86_64"
    echo ""

    print_info "Your platform: $OS-$ARCH"
    echo ""

    if [ "$OS" != "unknown" ] && [ "$ARCH" != "unknown" ]; then
        print_info "If your platform should be supported, this might be a download issue."
        print_info "Please report this at: https://github.com/edb-rs/edb/issues"
        echo ""
    fi

    print_info "Fallback option: Install from source (requires git and Rust toolchain)"
    print_info "⚠️  Warning: Building from source may take 10-20 minutes"
    echo ""

    # Check if running in interactive mode
    if [ -t 0 ]; then
        read -p "Do you want to proceed with source installation? (y/n) " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            echo ""
            print_info "Installation cancelled."
            print_info "You can try again later or report an issue if you believe"
            print_info "pre-built binaries should be available for your platform."
            exit 0
        fi
    else
        print_info "Non-interactive mode detected, proceeding with source installation..."
    fi

    echo ""
    print_info "=========================================="
    print_info "  Installing from source"
    print_info "=========================================="
    echo ""

    # Step 1: Check prerequisites
    print_info "Step 2/2: Installing from source..."
    echo ""
    print_info "Checking prerequisites..."
    check_git
    check_cargo
    check_bun
    echo ""

    # Step 2: Create ~/.edb directory
    print_info "Setting up installation directory..."
    create_edb_dir
    echo ""

    # Step 3: Clone repository
    print_info "Cloning EDB repository..."
    clone_repo
    echo ""

    # Step 4: Install components
    print_info "Building and installing EDB (this may take several minutes)..."
    install_edb
    echo ""

    # Ensure PATH includes cargo bin
    ensure_cargo_path

    # Success message
    echo ""
    print_success "=========================================="
    print_success "  EDB Installation Complete!"
    print_success "=========================================="
    echo ""
    print_info "You can now use EDB by running:"
    echo "  edb --help"
    echo ""
    print_info "To debug a transaction:"
    echo "  edb --rpc-urls <RPC_ENDPOINT> replay <TX_HASH>"
    echo ""
    print_info "For more information, visit:"
    echo "  https://github.com/edb-rs/edb"
    echo ""
}

# Run main installation
main
