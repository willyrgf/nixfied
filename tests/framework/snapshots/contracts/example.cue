package nixfied

import "list"
import "strings"

#machineOutput__result: close({
  kind: "task" | "workflow"
  metadata?: { [=~"^[a-z][a-z0-9_]*$"]: string }
  ok: bool
  output_path?: string & strings.MinRunes(1) & =~"^/.*$"
})
#runtime__event: (close({
  exit_code: int & >=0 & <=255
  kind: "passed"
  message?: (string & strings.MinRunes(1)) | null
})) | (close({
  kind: "queued"
  run_id: string & strings.MinRunes(1)
}))
#runtime__summary: close({
  kind: "workflow-summary"
  payload: #runtime__summary__payload
  version: 1
})
#runtime__summary__payload: close({
  attempt_id: string & strings.MinRunes(1)
  duration_ms?: (int & >=0) | null
  exit_code: int & >=0 & <=255
  labels?: { [=~"^[a-z][a-z0-9_]*$"]: string }
  run_id: string & strings.MinRunes(1)
  unit_ids: [...(string & strings.MinRunes(1))] & list.MinItems(1)
  workflow_id: string & strings.MinRunes(1)
})
