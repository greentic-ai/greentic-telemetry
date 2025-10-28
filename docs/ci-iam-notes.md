CI IAM Notes (OIDC / Federated)
================================

AWS
---

Trust policy: GitHub OIDC (restrict to your org/repo/branch).

Permissions (minimum):

- `logs:CreateLogGroup`, `logs:CreateLogStream`, `logs:PutLogEvents` on `/greentic/ci`
- `xray:PutTraceSegments`

Save role ARN as repo secret: `AWS_OIDC_ROLE_ARN`.

GCP
---

Workload Identity Federation provider: trust GitHub OIDC.

Service account roles:

- `roles/logging.logWriter`
- `roles/cloudtrace.agent`

Secrets:

- `GCP_PROJECT_ID`
- `GCP_WIF_PROVIDER`
- `GCP_SA_EMAIL`

Azure
-----

App registration with federated credentials for GitHub OIDC.

Application Insights (workspace-based or classic):

Instrumentation Key and App ID exposed as secrets:

- `AZURE_APPINSIGHTS_INSTRUMENTATION_KEY`
- `AZURE_APPINSIGHTS_APPID`

Additional secrets:

- `AZURE_CLIENT_ID`
- `AZURE_TENANT_ID`
- `AZURE_SUBSCRIPTION_ID`
