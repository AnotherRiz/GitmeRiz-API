# Health

[← Back to Index](README.md)

## GET /health

Public. Checks that the server is running.

Response `200`:
```json
{
  "status": "ok",
  "message": "Server is running"
}
```

```bash
curl http://localhost:3000/health
```
