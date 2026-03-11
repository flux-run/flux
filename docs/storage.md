# Storage

Fluxbase gives every project file columns backed by S3-compatible object storage. By default, files land in a Fluxbase-managed bucket — zero configuration required. For data ownership, cost transparency, or compliance requirements, you can connect your own bucket (BYO S3).

---

## Table of Contents

- [Overview](#overview)
- [How files work](#how-files-work)
- [Supported providers](#supported-providers)
- [Connecting a custom bucket](#connecting-a-custom-bucket)
  - [Dashboard](#dashboard)
  - [CLI (coming soon)](#cli-coming-soon)
- [Credential security](#credential-security)
- [Object key structure](#object-key-structure)
- [Presigned URLs](#presigned-urls)
- [Resetting to Fluxbase Managed](#resetting-to-fluxbase-managed)
- [API Reference](#api-reference)

---

## Overview

File columns (`fb_type: file`) store an object key — not raw bytes. Files are uploaded and downloaded via pre-signed URLs generated server-side. The data path is always:

```
client  ──presign──▶  Fluxbase API  ──returns URL──▶  client
client  ──PUT/GET──▶  S3 bucket  (direct, no Fluxbase proxy)
```

This means Fluxbase never sees the binary content of your files, and you pay S3 transfer costs directly to your cloud provider when using a custom bucket.

---

## How files work

1. Your application asks Fluxbase for a pre-signed URL for a specific table + column + row.
2. Fluxbase generates a signed URL pointing at your bucket (or its managed bucket).
3. The client uploads the file directly to S3 using an HTTP `PUT`.
4. Your application stores the returned **object key** on the row.
5. For private columns, download URLs are also pre-signed and expire after 15 minutes.

---

## Supported providers

| Provider | `provider` value | Endpoint required |
|---|---|---|
| Fluxbase Managed | `fluxbase` | No |
| Amazon S3 | `aws_s3` | No — region required |
| Cloudflare R2 | `r2` | Yes — `https://<accountid>.r2.cloudflarestorage.com` |
| DigitalOcean Spaces | `do_spaces` | Yes — `https://<region>.digitaloceanspaces.com` |
| MinIO / self-hosted | `minio` | Yes — your MinIO URL |
| Google Cloud Storage | `gcs` | Yes — `https://storage.googleapis.com` (S3 interop) |

All custom providers use the AWS S3 SDK with `force_path_style: true`, so any S3-compatible storage works.

---

## Connecting a custom bucket

### Dashboard

1. Open **Storage → Storage Provider** in the sidebar.
2. Click the **Storage Provider** dropdown and select your provider.
3. Fill in the required fields:
   - **Bucket name** — the name of the bucket to store files in.
   - **Region** — required for AWS S3; optional for endpoint-based providers.
   - **Endpoint URL** — required for R2, Spaces, MinIO, GCS.
   - **Base path** — optional prefix inside the bucket (e.g. `prod/uploads`). Useful for multi-environment setups sharing a single bucket.
   - **Access key ID** / **Secret access key** — IAM credentials with `s3:PutObject`, `s3:GetObject`, `s3:DeleteObject`, `s3:ListBucket` on your bucket.
4. Click **Connect custom bucket**.

The configuration takes effect immediately — new presigned URLs use your bucket.

### CLI (coming soon)

```bash
# Connect an AWS S3 bucket
flux storage set \
  --provider aws_s3 \
  --bucket my-company-files \
  --region us-east-1 \
  --access-key-id AKIAIOSFODNN7EXAMPLE \
  --secret-access-key wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY

# Connect Cloudflare R2
flux storage set \
  --provider r2 \
  --bucket my-r2-bucket \
  --endpoint https://abc123.r2.cloudflarestorage.com \
  --access-key-id <r2-access-key-id> \
  --secret-access-key <r2-secret>

# View current config
flux storage show

# Reset to Fluxbase Managed
flux storage reset
```

### Required IAM permissions (AWS S3 example)

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "s3:PutObject",
        "s3:GetObject",
        "s3:DeleteObject",
        "s3:ListBucket"
      ],
      "Resource": [
        "arn:aws:s3:::my-company-files",
        "arn:aws:s3:::my-company-files/*"
      ]
    }
  ]
}
```

---

## Credential security

Credentials (access key ID and secret access key) are encrypted at rest using **AES-256-GCM** (the same scheme used for project secrets). They are:

- Never returned in API responses — GET `/storage/provider` always returns `"***"` for credential fields.
- Never logged or included in error messages.
- Decrypted only in memory at presign time.

If you update your bucket configuration without re-entering credentials, the existing encrypted values are preserved (`COALESCE` upsert).

---

## Object key structure

Every file stored by Fluxbase follows this path:

```
{base_path}/{tenant_id}/{project_id}/{table}/{row_id}/{column}/{uuid}
```

| Segment | Description |
|---|---|
| `base_path` | Optional prefix you configure (e.g. `prod/uploads`) |
| `tenant_id` | Your Fluxbase tenant UUID |
| `project_id` | The project UUID |
| `table` | Table name where the file column lives |
| `row_id` | UUID of the row this file belongs to |
| `column` | Column name (e.g. `avatar`) |
| `uuid` | Random UUID generated per upload — prevents collisions on re-upload |

Example key:
```
prod/uploads/t_01abc/p_xyz789/users/row_aaa/avatar/550e8400-e29b-41d4-a716.jpg
```

---

## Presigned URLs

### Upload URL

```http
POST /v1/projects/{project_id}/storage/presign
Authorization: Bearer <token>
Content-Type: application/json

{
  "table": "users",
  "column": "avatar",
  "row_id": "550e8400-e29b-41d4-a716-446655440000",
  "kind": "upload"
}
```

Response:
```json
{
  "url": "https://my-bucket.s3.us-east-1.amazonaws.com/prod/users/...",
  "key": "prod/uploads/t_01abc/p_xyz789/users/550e8400.../avatar/uuid.bin",
  "expires_in": 900,
  "bucket": "my-bucket"
}
```

Use the `url` with `PUT` (no auth header needed — it's built into the signed URL). Store the returned `key` on the row.

### Download URL

Same request with `"kind": "download"`. The response URL is a `GET` pre-signed URL valid for 15 minutes.

For **public** file columns, the URL is a direct (unsigned) S3 URL that never expires.

---

## Resetting to Fluxbase Managed

Resetting removes your custom provider configuration. Files already uploaded to your bucket are **not deleted** — only new uploads will go to the Fluxbase-managed bucket.

**Dashboard:** Storage → Storage Provider → **Reset** button.

**API:**
```http
DELETE /v1/projects/{project_id}/storage/provider
Authorization: Bearer <token>
```

Returns `204 No Content`.

---

## API Reference

All routes are project-scoped and require authentication.

### GET `/storage/provider`

Returns the current storage configuration. Credentials are always redacted as `"***"`.

**Response (Fluxbase managed):**
```json
{
  "provider": "fluxbase",
  "is_active": true,
  "is_custom": false
}
```

**Response (custom):**
```json
{
  "id": "...",
  "provider": "aws_s3",
  "bucket_name": "my-company-files",
  "region": "us-east-1",
  "endpoint_url": null,
  "base_path": "prod/uploads",
  "access_key_id": "***",
  "secret_access_key": "***",
  "is_active": true,
  "is_custom": true,
  "created_at": "2026-03-11T00:00:00Z",
  "updated_at": "2026-03-11T00:00:00Z"
}
```

---

### PUT `/storage/provider`

Create or update the custom storage configuration. Only fields you include are updated; omitting `access_key_id` or `secret_access_key` preserves existing encrypted values.

**Body:**
```json
{
  "provider": "r2",
  "bucket_name": "my-r2-bucket",
  "region": null,
  "endpoint_url": "https://abc123.r2.cloudflarestorage.com",
  "base_path": "uploads",
  "access_key_id": "...",
  "secret_access_key": "..."
}
```

- `provider` must be one of: `fluxbase`, `aws_s3`, `r2`, `gcs`, `minio`, `do_spaces`
- For non-`fluxbase` providers, `bucket_name`, `access_key_id`, and `secret_access_key` are required on first save.
- Returns `200` with the saved config (credentials redacted).
- Returns `422` if provider is invalid.

---

### DELETE `/storage/provider`

Removes the custom configuration. Returns `204 No Content`.

---

### POST `/storage/presign`

Generate a pre-signed URL for a file upload or download.

**Body:**
```json
{
  "table": "documents",
  "column": "attachment",
  "row_id": "uuid-of-row",
  "kind": "upload"
}
```

- `kind`: `"upload"` → returns a `PUT` URL · `"download"` → returns a `GET` URL
- Returns `200` with `{ url, key, expires_in, bucket }`.
