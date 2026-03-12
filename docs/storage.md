# Storage

Flux gives every project file columns backed by S3-compatible object storage.
You configure your own S3-compatible bucket.

---

## How files work

1. Your function asks for a presigned URL for a specific table + column + row
2. Flux generates a signed URL pointing at your bucket
3. The client uploads/downloads directly to S3 (Flux never proxies binary data)
4. The object key is stored on the row

```
client  ──presign──▶  Data Engine  ──returns URL──▶  client
client  ──PUT/GET──▶  S3 bucket  (direct, no Flux proxy)
```

---

## Supported providers

| Provider | Config value | Notes |
|---|---|---|
| Amazon S3 | `aws_s3` | Region required |
| Cloudflare R2 | `r2` | Endpoint required |
| DigitalOcean Spaces | `do_spaces` | Endpoint required |
| MinIO / self-hosted | `minio` | Endpoint required |
| Google Cloud Storage | `gcs` | S3 interop endpoint |

All providers use the AWS S3 SDK with `force_path_style: true`, so any
S3-compatible storage works.

---

## Configuration

### flux.toml

```toml
[storage]
provider   = "minio"
bucket     = "my-app-files"
endpoint   = "http://localhost:9000"
region     = "us-east-1"
base_path  = "uploads"
```

### CLI

```bash
flux storage set \
  --provider aws_s3 \
  --bucket my-files \
  --region us-east-1 \
  --access-key-id AKIA... \
  --secret-access-key wJal...

flux storage show    # view current config
flux storage reset   # reset to default
```

### Environment variables

| Env var | Description |
|---|---|
| `STORAGE_PROVIDER` | Provider type |
| `STORAGE_BUCKET` | Bucket name |
| `STORAGE_ENDPOINT` | S3 endpoint URL |
| `STORAGE_REGION` | AWS region |
| `STORAGE_ACCESS_KEY_ID` | IAM access key |
| `STORAGE_SECRET_ACCESS_KEY` | IAM secret key |
| `STORAGE_BASE_PATH` | Optional prefix inside bucket |

---

## Object key structure

```
{tenant_id}/{project_id}/{table}/{column}/{row_id}/{filename}
```

For self-hosted without multi-tenancy:
```
{project}/{table}/{column}/{row_id}/{filename}
```

---

## Presigned URLs

- **Upload:** `PUT` presigned URL, expires in 15 minutes
- **Download (public columns):** Direct URL
- **Download (private columns):** `GET` presigned URL, expires in 15 minutes

---

## Credential security

Storage credentials are encrypted at rest using the same AES-256-GCM system
as secrets. They are never exposed in API responses, logs, or execution records.

---

*For the overall framework architecture, see [framework.md](framework.md).*
