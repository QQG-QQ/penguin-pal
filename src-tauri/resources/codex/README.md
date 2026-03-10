Embedded Codex runtime files live here.

Expected Windows layout:

codex/
  windows-x64/
    node_modules/
      .bin/
        codex.cmd
        node.exe
      @openai/
        codex/

This runtime is private to PenguinPal. It should be bundled with the app and is
resolved before any system-wide `codex` installation.
