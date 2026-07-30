# sim-expr-tree

Thin product binary for the expression-tree engine, reversible view,
authoritative server, and generic web host. The binary only chooses the
`cli/main/expr-tree` host verb and delegates bootstrap, runtime configuration,
capability setup, and dispatch to `sim-run-core`.

```sh
cargo run -p sim-expr-tree -- --help
cargo run -p sim-expr-tree -- --config-file expr-tree.toml
```

Product settings live in the standard runtime config table:

```toml
[lib/expr-tree-serve]
placement = "in-process"
storage = "expression-tree"
browser-resource = "tree"
web-addr = "127.0.0.1:8787"
atelier-root = ".sim/atelier"
```

Set `placement = "external"` and `server-site` to an already loaded
`EvalFabric` site to place the authoritative server outside the product
process. The product still uses the existing fabric request and transport
contracts; it does not add a server protocol or product-specific CLI parser.
