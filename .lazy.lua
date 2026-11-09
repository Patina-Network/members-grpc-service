-- Avoids `Box` hovers from pinned async-trait (see Cargo.toml).
return {
  {
    "mrcjkb/rustaceanvim",
    opts = {
      server = {
        default_settings = {
          ["rust-analyzer"] = {
            procMacro = {
              ignored = {
                ["async-trait"] = { "async_trait" },
              },
            },
            -- Silences the hint the ignore above causes.
            diagnostics = {
              disabled = { "proc-macro-disabled" },
            },
          },
        },
      },
    },
  },
}
