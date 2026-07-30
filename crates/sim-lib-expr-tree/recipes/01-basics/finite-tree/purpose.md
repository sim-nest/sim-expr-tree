# Finite mixed-backend expression tree

This recipe opens one finite tree, uses durable generated directory and cell
names, and mounts a database Dir beside a read-only Table. It resolves cells by
bare, relative, and absolute canonical Table path forms. The checked cargo test
runs this exact Lisp form and verifies the generated paths, values, mount
constraints, and bounded root listing.
