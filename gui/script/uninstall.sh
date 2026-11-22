#!/usr/bin/env sh
set -eu

# Uninstalls Obake that was installed using the install.sh script

check_remaining_installations() {
    platform="$(uname -s)"
    if [ "$platform" = "Darwin" ]; then
        # Check for any Obake variants in /Applications
        remaining=$(ls -d /Applications/Obake*.app 2>/dev/null | wc -l)
        [ "$remaining" -eq 0 ]
    else
        # Check for any Obake variants in ~/.local
        remaining=$(ls -d "$HOME/.local/obake"*.app 2>/dev/null | wc -l)
        [ "$remaining" -eq 0 ]
    fi
}

prompt_remove_preferences() {
    printf "Do you want to keep your Obake preferences? [Y/n] "
    read -r response
    case "$response" in
        [nN]|[nN][oO])
            rm -rf "$HOME/.config/obake"
            echo "Preferences removed."
            ;;
        *)
            echo "Preferences kept."
            ;;
    esac
}

main() {
    platform="$(uname -s)"
    channel="${OBAKE_CHANNEL:-stable}"

    if [ "$platform" = "Darwin" ]; then
        platform="macos"
    elif [ "$platform" = "Linux" ]; then
        platform="linux"
    else
        echo "Unsupported platform $platform"
        exit 1
    fi

    "$platform"

    echo "Obake has been uninstalled"
}

linux() {
    suffix=""
    if [ "$channel" != "stable" ]; then
        suffix="-$channel"
    fi

    appid=""
    db_suffix="stable"
    case "$channel" in
      stable)
        appid="dev.obake.Obake"
        db_suffix="stable"
        ;;
      nightly)
        appid="dev.obake.Obake-Nightly"
        db_suffix="nightly"
        ;;
      preview)
        appid="dev.obake.Obake-Preview"
        db_suffix="preview"
        ;;
      dev)
        appid="dev.obake.Obake-Dev"
        db_suffix="dev"
        ;;
      *)
        echo "Unknown release channel: ${channel}. Using stable app ID."
        appid="dev.obake.Obake"
        db_suffix="stable"
        ;;
    esac

    # Remove the app directory
    rm -rf "$HOME/.local/obake$suffix.app"

    # Remove the binary symlink
    rm -f "$HOME/.local/bin/obake"

    # Remove the .desktop file
    rm -f "$HOME/.local/share/applications/${appid}.desktop"

    # Remove the database directory for this channel
    rm -rf "$HOME/.local/share/obake/db/0-$db_suffix"

    # Remove socket file
    rm -f "$HOME/.local/share/obake/obake-$db_suffix.sock"

    # Remove the entire Obake directory if no installations remain
    if check_remaining_installations; then
        rm -rf "$HOME/.local/share/obake"
        prompt_remove_preferences
    fi

    rm -rf $HOME/.obake_server
}

macos() {
    app="Obake.app"
    db_suffix="stable"
    app_id="dev.obake.Obake"
    case "$channel" in
      nightly)
        app="Obake Nightly.app"
        db_suffix="nightly"
        app_id="dev.obake.Obake-Nightly"
        ;;
      preview)
        app="Obake Preview.app"
        db_suffix="preview"
        app_id="dev.obake.Obake-Preview"
        ;;
      dev)
        app="Obake Dev.app"
        db_suffix="dev"
        app_id="dev.obake.Obake-Dev"
        ;;
    esac

    # Remove the app bundle
    if [ -d "/Applications/$app" ]; then
        rm -rf "/Applications/$app"
    fi

    # Remove the binary symlink
    rm -f "$HOME/.local/bin/obake"

    # Remove the database directory for this channel
    rm -rf "$HOME/Library/Application Support/Obake/db/0-$db_suffix"

    # Remove app-specific files and directories
    rm -rf "$HOME/Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments/$app_id.sfl"*
    rm -rf "$HOME/Library/Caches/$app_id"
    rm -rf "$HOME/Library/HTTPStorages/$app_id"
    rm -rf "$HOME/Library/Preferences/$app_id.plist"
    rm -rf "$HOME/Library/Saved Application State/$app_id.savedState"

    # Remove the entire Obake directory if no installations remain
    if check_remaining_installations; then
        rm -rf "$HOME/Library/Application Support/Obake"
        rm -rf "$HOME/Library/Logs/Obake"

        prompt_remove_preferences
    fi

    rm -rf $HOME/.obake_server
}

main "$@"
