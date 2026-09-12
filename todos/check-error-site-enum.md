# Replace the outputs/for_each sentinel sites in CheckError with a site enum

**Filed:** 2026-09-12 (checker adversarial pass 1, confirmed by pass 2)

Errors on a workflow output binding use the sentinel node name `outputs`, and errors on a
`for_each` binding use the sentinel port name `for_each`. Three bugs so far came from the
collision with real nodes or ports of those names. When milestone 2 adds composite output
ports for workflow-as-tool, introduce `Site::{Port { node, port }, ForEach { node },
Output { name } }` and change every variant's fields accordingly.
