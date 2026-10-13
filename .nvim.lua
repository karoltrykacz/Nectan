vim.g.project_color = "#83A598"

vim.lsp.config("rust_analyzer", {
	settings = {
		["rust-analyzer"] = {
			rustfmt = { extraArgs = { "+nightly" } },
		},
	},
})
