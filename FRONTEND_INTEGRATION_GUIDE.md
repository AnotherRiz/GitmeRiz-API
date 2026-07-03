# Frontend Integration Guide - Background Processing

This guide helps frontend developers integrate with the new background processing upload system.

## Breaking Changes

### Upload Response Changed
**Before:** `201 Created` with fully processed items
```json
{
  "success": true,
  "data": {
    "id": 123,
    "status": "active",
    "thumbnail_path": "/gallery/.../thumb.webp",
    "preview_path": "/gallery/.../preview.webp"
  }
}
```

**After:** `202 Accepted` with items in processing state
```json
{
  "success": true,
  "data": {
    "id": 123,
    "status": "processing",  // ← Not ready yet!
    "thumbnail_path": null,  // ← Will be generated
    "preview_path": null     // ← Will be generated
  }
}
```

## Implementation Steps

### Step 1: Handle 202 Accepted

Update your upload handler to accept both 201 and 202:

```typescript
async function uploadImages(files: File[]) {
  const formData = new FormData();
  files.forEach(file => formData.append('file', file));
  
  const response = await fetch('/api/gallery', {
    method: 'POST',
    body: formData
  });
  
  // Accept both 201 (legacy) and 202 (new)
  if (response.status === 201 || response.status === 202) {
    const data = await response.json();
    return data.data; // Single item or array
  }
  
  throw new Error('Upload failed');
}
```

### Step 2: Implement Status Polling

Poll the status endpoint for processing items:

```typescript
interface GalleryItem {
  id: number;
  status: 'processing' | 'active' | 'failed_processing';
  thumbnail_path: string | null;
  preview_path: string | null;
  // ... other fields
}

async function pollStatus(
  items: GalleryItem[],
  onUpdate: (id: number, status: string) => void
): Promise<void> {
  const processingIds = items
    .filter(item => item.status === 'processing')
    .map(item => item.id);
  
  if (processingIds.length === 0) {
    return; // All done
  }
  
  const response = await fetch('/api/gallery/status', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ids: processingIds })
  });
  
  const { data } = await response.json();
  
  // Update each item
  for (const [id, status] of Object.entries(data)) {
    onUpdate(Number(id), status as string);
  }
}
```

### Step 3: Set Up Polling Interval

```typescript
function startPolling(
  items: GalleryItem[],
  updateItem: (id: number, status: string) => void
) {
  const intervalId = setInterval(async () => {
    await pollStatus(items, updateItem);
    
    // Stop if all items are processed
    const allProcessed = items.every(
      item => item.status === 'active' || item.status === 'failed_processing'
    );
    
    if (allProcessed) {
      clearInterval(intervalId);
    }
  }, 2000); // Poll every 2 seconds
  
  // Cleanup on unmount
  return () => clearInterval(intervalId);
}
```

### Step 4: UI States

Show different UI based on status:

```tsx
function GalleryItemCard({ item }: { item: GalleryItem }) {
  if (item.status === 'processing') {
    return (
      <div className="processing">
        <Skeleton />
        <Spinner />
        <span>Processing...</span>
      </div>
    );
  }
  
  if (item.status === 'failed_processing') {
    return (
      <div className="failed">
        <AlertIcon />
        <span>Processing failed</span>
        <button onClick={() => retryProcessing(item.id)}>
          Retry
        </button>
      </div>
    );
  }
  
  // status === 'active'
  return (
    <img 
      src={`/api/gallery/t/${item.short_id}`}
      alt={item.title}
    />
  );
}
```

### Step 5: Retry Failed Items

```typescript
async function retryProcessing(imageId: number) {
  const response = await fetch(`/api/gallery/${imageId}/reprocess`, {
    method: 'POST'
  });
  
  if (response.status === 202) {
    // Reprocess started, poll again
    // Set item status back to 'processing'
    return true;
  }
  
  return false;
}
```

## Complete React Example

```tsx
import { useState, useEffect } from 'react';

function GalleryUpload() {
  const [items, setItems] = useState<GalleryItem[]>([]);
  const [uploading, setUploading] = useState(false);
  
  // Handle file upload
  async function handleUpload(files: FileList) {
    setUploading(true);
    
    try {
      const uploadedItems = await uploadImages(Array.from(files));
      setItems(prev => [...prev, ...uploadedItems]);
    } finally {
      setUploading(false);
    }
  }
  
  // Update item status
  function updateItemStatus(id: number, status: string) {
    setItems(prev => prev.map(item => 
      item.id === id ? { ...item, status } : item
    ));
  }
  
  // Poll for processing items
  useEffect(() => {
    const processingItems = items.filter(i => i.status === 'processing');
    
    if (processingItems.length === 0) {
      return;
    }
    
    const intervalId = setInterval(async () => {
      await pollStatus(processingItems, updateItemStatus);
    }, 2000);
    
    return () => clearInterval(intervalId);
  }, [items]);
  
  return (
    <div>
      <input 
        type="file" 
        multiple 
        onChange={e => e.target.files && handleUpload(e.target.files)}
        disabled={uploading}
      />
      
      <div className="gallery-grid">
        {items.map(item => (
          <GalleryItemCard key={item.id} item={item} />
        ))}
      </div>
    </div>
  );
}
```

## Complete Vue Example

```vue
<script setup lang="ts">
import { ref, watch } from 'vue';

const items = ref<GalleryItem[]>([]);
const uploading = ref(false);

async function handleUpload(files: FileList) {
  uploading.value = true;
  
  try {
    const uploadedItems = await uploadImages(Array.from(files));
    items.value.push(...uploadedItems);
  } finally {
    uploading.value = false;
  }
}

function updateItemStatus(id: number, status: string) {
  const item = items.value.find(i => i.id === id);
  if (item) {
    item.status = status;
  }
}

// Watch for processing items and poll
watch(items, (newItems) => {
  const processingItems = newItems.filter(i => i.status === 'processing');
  
  if (processingItems.length === 0) return;
  
  const intervalId = setInterval(async () => {
    await pollStatus(processingItems, updateItemStatus);
  }, 2000);
  
  // Cleanup
  return () => clearInterval(intervalId);
}, { deep: true });
</script>

<template>
  <div>
    <input 
      type="file" 
      multiple 
      @change="e => e.target.files && handleUpload(e.target.files)"
      :disabled="uploading"
    />
    
    <div class="gallery-grid">
      <GalleryItemCard 
        v-for="item in items" 
        :key="item.id" 
        :item="item"
      />
    </div>
  </div>
</template>
```

## Performance Best Practices

### 1. Batch Status Checks
Don't check status individually:
```typescript
// ❌ Bad: Multiple requests
for (const id of processingIds) {
  await fetch(`/api/gallery/${id}/status`);
}

// ✅ Good: Single batch request
await fetch('/api/gallery/status', {
  body: JSON.stringify({ ids: processingIds })
});
```

### 2. Debounce Polling
If user navigates away and comes back, don't create duplicate intervals:
```typescript
let pollingInterval: NodeJS.Timeout | null = null;

function startPolling() {
  if (pollingInterval) {
    clearInterval(pollingInterval);
  }
  
  pollingInterval = setInterval(pollStatus, 2000);
}
```

### 3. Stop Polling When Not Visible
Save bandwidth when tab is not active:
```typescript
document.addEventListener('visibilitychange', () => {
  if (document.hidden) {
    clearInterval(pollingInterval);
  } else {
    startPolling();
  }
});
```

### 4. Show Upload Progress
Even though processing is async, show upload progress:
```typescript
const xhr = new XMLHttpRequest();

xhr.upload.addEventListener('progress', (e) => {
  if (e.lengthComputable) {
    const percentComplete = (e.loaded / e.total) * 100;
    setUploadProgress(percentComplete);
  }
});
```

## Error Handling

### Network Errors
```typescript
async function pollStatusWithRetry(
  items: GalleryItem[],
  maxRetries = 3
) {
  for (let i = 0; i < maxRetries; i++) {
    try {
      await pollStatus(items, updateItemStatus);
      return;
    } catch (error) {
      if (i === maxRetries - 1) {
        console.error('Failed to poll status after retries', error);
      }
      await new Promise(resolve => setTimeout(resolve, 1000 * (i + 1)));
    }
  }
}
```

### Item Not Found
```typescript
// Status check returns empty for items user doesn't own
const { data } = await response.json();

for (const item of processingItems) {
  if (!(item.id in data)) {
    // Item not found or access denied
    updateItemStatus(item.id, 'failed_processing');
  }
}
```

## Testing Checklist

- [ ] Upload single image
- [ ] Upload multiple images (2-5)
- [ ] Upload bulk images (10+)
- [ ] Navigate away during processing
- [ ] Reload page during processing (items should show as processing)
- [ ] Retry failed processing
- [ ] Handle network interruption during upload
- [ ] Handle network interruption during polling
- [ ] Multiple tabs with same gallery
- [ ] Mobile device (slow internet)

## Migration Guide

If you have existing frontend code:

1. **Update upload handler** to handle 202 Accepted
2. **Add polling logic** for processing items
3. **Update UI components** to show loading/error states
4. **Test thoroughly** with different scenarios

### Backward Compatibility
The backend may still return 201 Created in some cases. Your frontend should handle both:

```typescript
if (response.status === 201) {
  // Old behavior: item is ready immediately
  addToGallery(item);
} else if (response.status === 202) {
  // New behavior: item needs processing
  addToGallery(item);
  startPolling([item]);
}
```

## Optional: WebSocket (Future Enhancement)

For more efficient real-time updates, you can use WebSocket instead of polling:

```typescript
const ws = new WebSocket('ws://localhost:3000/gallery/watch');

ws.onopen = () => {
  // Subscribe to specific items
  ws.send(JSON.stringify({
    action: 'subscribe',
    imageIds: [123, 124, 125]
  }));
};

ws.onmessage = (event) => {
  const { imageId, status } = JSON.parse(event.data);
  updateItemStatus(imageId, status);
};
```

This is not yet implemented on the backend but is a recommended future enhancement.

## Support

For questions or issues:
1. Check backend logs for errors
2. Verify authentication tokens
3. Ensure `/api/gallery/status` endpoint is accessible
4. Test with Postman/curl first before debugging frontend

---

Last updated: 2026-07-03
