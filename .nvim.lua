vim.g.project_color = "#0083f5"

vim.lsp.config("rust_analyzer", {
	settings = {
		["rust-analyzer"] = {
			rustfmt = { extraArgs = { "+nightly" } },
		},
	},
})
