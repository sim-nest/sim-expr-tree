# sim-expr-tree-core

Backend-neutral finite namespace records for expression-tree storage. The crate
defines stable tree, directory, and cell identities; durable source and stamp
records; field-wise inherited source/result codec policy and hard-clamped face
budgets; and serialized generated-name reservation over canonical
`sim-table-core` path names.

The storage model keeps authored source, operational control, and rebuildable
derived state in separate lanes while composing explicit Table and Dir mounts.
The calculation crate adapts the derived lane to an ordinary `Table`, allowing
each selected backend to retain its own durability and transaction behavior.
