# Background Processing Design

## Konsep: Upload Instan dengan Background Processing

Ide yang kamu ajukan adalah **Optimization 3** dari roadmap: memberikan respons instan kepada user dan memproses gambar di background.

## Flow Baru

### 1. Upload Flow (User Perspective)
```
User → Upload files → Server terima → 202 Accepted (instant!) → User bisa leave page
                                    ↓
                            Background processing...
                                    ↓
                            Status: processing → active
```

### 2. Technical Flow

#### Phase 1: Fast Upload (< 200ms)
1. Terima multipart files dari user
2. Validasi extension dan size
3. **Simpan file mentah ke disk** (I/O cepat)
4. **Insert ke database dengan `status='processing'`** (thumbnail_path = NULL)
5. **Return 202 Accepted** dengan data items (status masih 'processing')

#### Phase 2: Background Processing (detached task)
1. `tokio::spawn()` tanpa `.await` → user sudah dapat response
2. Baca file mentah dari disk
3. Generate thumbnail + preview dengan semaphore
4. Save thumbnail + preview in parallel
5. Update database: `SET status='active', thumbnail_path=?, preview_path=?`

#### Phase 3: Status Tracking
Frontend perlu polling atau WebSocket untuk cek status:
```typescript
// Frontend polling example
async function checkStatus(imageIds: number[]) {
  const response = await fetch('/api/gallery/status', {
    method: 'POST',
    body: JSON.stringify({ ids: imageIds })
  });
  return response.json(); // { "123": "active", "124": "processing" }
}
```

## Keuntungan

### User Experience
- ✅ **Upload terasa instant** (< 200ms vs 450ms)
- ✅ **User bisa langsung leave page** (tidak perlu nungguin loading)
- ✅ **Skalabel untuk bulk upload** (100 gambar = tetap < 200ms response)
- ✅ **Internet cepat = maksimal utilized** (upload selesai cepat)

### Technical
- ✅ HTTP connection tidak blocked
- ✅ Server bisa handle lebih banyak concurrent uploads
- ✅ Retry logic mudah (tinggal call reprocess endpoint)
- ✅ Monitoring jelas (status column di database)

## Challenges & Solutions

### 1. Frontend harus track status
**Solution:**
```typescript
// Polling setiap 2 detik untuk items yang processing
setInterval(async () => {
  const processing = items.filter(i => i.status === 'processing');
  if (processing.length > 0) {
    const statuses = await checkStatus(processing.map(i => i.id));
    updateItemStatuses(statuses);
  }
}, 2000);
```

### 2. Server crash saat background processing
**Solution:** Cleanup job yang jalan periodik
```rust
// Cleanup stuck items (status='processing' > 10 minutes)
sqlx::query(
  "UPDATE gallery SET status='failed_processing' 
   WHERE status='processing' AND created_at < NOW() - INTERVAL 10 MINUTE"
).execute(&pool).await;
```

### 3. Error visibility
**Solution:** Frontend show badge/icon untuk failed_processing
```tsx
{item.status === 'failed_processing' && (
  <button onClick={() => reprocess(item.id)}>
    Retry Processing
  </button>
)}
```

### 4. Database row tanpa thumbnail (orphaned)
**Solution:** Status column + reprocess endpoint sudah handle ini

## API Changes

### Upload Response (202 Accepted)
```json
{
  "success": true,
  "data": {
    "id": 123,
    "title": "My Image",
    "status": "processing",
    "thumbnail_path": null,  // Belum ada
    "preview_path": null,    // Belum ada
    "short_id": "AbCd1234",
    ...
  }
}
```

### Status Check Endpoint (NEW)
```
POST /api/gallery/status
Body: { "ids": [123, 124, 125] }

Response:
{
  "success": true,
  "data": {
    "123": "active",
    "124": "processing",
    "125": "failed_processing"
  }
}
```

### Reprocess Endpoint (EXISTING)
```
POST /api/gallery/{id}/reprocess

Response: 202 Accepted (also runs in background)
```

## Implementation Checklist

### Backend
- [ ] Refactor upload endpoint untuk return 202 Accepted
- [ ] Spawn detached tokio task untuk background processing
- [ ] Create status check endpoint `POST /gallery/status`
- [ ] Add cleanup job untuk stuck processing items
- [ ] Update API docs dengan perubahan status code

### Frontend
- [ ] Handle 202 Accepted response
- [ ] Implement polling untuk items dengan status='processing'
- [ ] Show loading spinner/skeleton untuk items yang processing
- [ ] Show retry button untuk items dengan failed_processing
- [ ] Stop polling saat semua items sudah active

### Optional Enhancements
- [ ] WebSocket untuk real-time updates (lebih efisien dari polling)
- [ ] Progress tracking per image (0-100%)
- [ ] Batch status endpoint untuk efisiensi

## Performance Comparison

| Metric | Current (Blocking) | New (Background) |
|--------|-------------------|------------------|
| **Response time** | 450ms | < 200ms |
| **User wait time** | 450ms | 0ms (instant) |
| **Bulk (50 images)** | ~3 seconds | < 200ms |
| **Server capacity** | Limited by processing | Much higher |
| **User experience** | Must wait | Can leave page |

## Risiko & Mitigasi

### Risiko 1: User tidak tahu processing masih jalan
**Mitigasi:** Frontend show clear UI state (loading, spinner, badge)

### Risiko 2: Database penuh items dengan status='processing'
**Mitigasi:** Cleanup job + monitoring alert

### Risiko 3: Background task gagal tanpa user tahu
**Mitigasi:** Failed status + retry button + email notification (optional)

## Kesimpulan

Implementasi background processing adalah **upgrade signifikan** untuk UX, terutama untuk:
- Bulk uploads (10+ images)
- Users dengan internet cepat
- Mobile users yang sering switch apps

Trade-off:
- Frontend lebih kompleks (perlu polling/WebSocket)
- Backend lebih kompleks (background job management)
- Tapi **UX improvement sangat worth it!**

## Next Steps

1. **Implement status check endpoint** (paling mudah, butuh 30 menit)
2. **Refactor upload to background** (butuh 2-3 jam)
3. **Frontend polling** (butuh 1-2 jam)
4. **Testing & monitoring** (butuh 2-4 jam)

Total effort: **1-2 hari kerja** untuk full implementation.
