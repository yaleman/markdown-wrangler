class EditorStorage {
    constructor(filePath) {
        this.filePath = filePath;
        this.storageKey = `markdown-wrangler-${filePath}`;
        this.timestampKey = `${this.storageKey}-timestamp`;
        this.originalContentKey = `${this.storageKey}-original`;
        this.checkInterval = 5000; // Check every 5 seconds
        this.intervalId = null;
        
        this.initializeStorage();
        this.startPeriodicCheck();
    }

    initializeStorage() {
        // Store original content and current server timestamp when editor loads
        const textarea = document.querySelector('textarea[name="content"]');
        if (textarea) {
            const originalContent = textarea.value;
            localStorage.setItem(this.originalContentKey, originalContent);
            
            // Get current server timestamp
            this.updateServerTimestamp().then(() => {
                // Load any existing draft from local storage
                this.loadDraft();
            });
        }
    }

    async updateServerTimestamp() {
        try {
            const response = await fetch(`/file-info?path=${encodeURIComponent(this.filePath)}`);
            if (response.ok) {
                const data = await response.json();
                localStorage.setItem(this.timestampKey, data.modified_time);
                return data.modified_time;
            }
        } catch (error) {
            console.error('Failed to get server timestamp:', error);
        }
        return null;
    }

    saveDraft() {
        const textarea = document.querySelector('textarea[name="content"]');
        if (textarea) {
            const content = textarea.value;
            const timestamp = new Date().toISOString();
            
            const draft = {
                content: content,
                saved_at: timestamp,
                file_path: this.filePath
            };
            
            localStorage.setItem(this.storageKey, JSON.stringify(draft));
            
            // Update UI to show draft saved
            this.showDraftStatus('Draft saved locally');
        }
    }

    loadDraft() {
        const draftJson = localStorage.getItem(this.storageKey);
        if (draftJson) {
            try {
                const draft = JSON.parse(draftJson);
                const textarea = document.querySelector('textarea[name="content"]');
                const originalContent = localStorage.getItem(this.originalContentKey);
                
                // Only load draft if it's different from original
                if (textarea && draft.content !== originalContent) {
                    const timeDiff = new Date() - new Date(draft.saved_at);
                    const hours = Math.floor(timeDiff / (1000 * 60 * 60));
                    
                    if (confirm(`Found unsaved draft from ${hours > 0 ? hours + ' hours' : 'less than an hour'} ago. Load draft?`)) {
                        textarea.value = draft.content;
                        this.updatePreview();
                        this.showDraftStatus('Draft loaded from local storage');
                    }
                }
            } catch (error) {
                console.error('Failed to load draft:', error);
            }
        }
    }

    async checkForUpdates() {
        try {
            const currentServerTimestamp = await this.updateServerTimestamp();
            const storedTimestamp = localStorage.getItem(this.timestampKey);
            
            if (currentServerTimestamp && storedTimestamp && currentServerTimestamp !== storedTimestamp) {
                // File has been modified on disk
                this.handleFileConflict(currentServerTimestamp);
            }
        } catch (error) {
            console.error('Failed to check for updates:', error);
        }
    }

    handleFileConflict(newTimestamp) {
        this.stopPeriodicCheck(); // Stop checking to avoid multiple prompts
        
        const message = `The file has been modified on disk by another process.\n\nWhat would you like to do?`;
        const choice = confirm(`${message}\n\nClick OK to reload the file (losing local changes)\nClick Cancel to keep editing (you can save to overwrite)`);
        
        if (choice) {
            // Reload file from server
            this.reloadFromServer();
        } else {
            // Keep local version, update stored timestamp to prevent repeated prompts
            localStorage.setItem(this.timestampKey, newTimestamp);
            this.showDraftStatus('Warning: File modified on disk - local changes will overwrite when saved', 'warning');
        }
        
        // Restart checking after handling conflict
        setTimeout(() => this.startPeriodicCheck(), 10000); // Wait 10 seconds before resuming
    }

    async reloadFromServer() {
        try {
            const response = await fetch(`/file-content?path=${encodeURIComponent(this.filePath)}`);
            if (response.ok) {
                const data = await response.json();
                const textarea = document.querySelector('textarea[name="content"]');
                if (textarea) {
                    textarea.value = data.content;
                    this.updatePreview();
                    
                    // Update stored content and clear draft
                    localStorage.setItem(this.originalContentKey, data.content);
                    localStorage.removeItem(this.storageKey);
                    
                    this.showDraftStatus('File reloaded from disk');
                }
            }
        } catch (error) {
            console.error('Failed to reload file:', error);
            this.showDraftStatus('Error reloading file', 'error');
        }
    }

    clearDraft() {
        localStorage.removeItem(this.storageKey);
        localStorage.removeItem(this.timestampKey);
        localStorage.removeItem(this.originalContentKey);
        this.stopPeriodicCheck();
    }

    startPeriodicCheck() {
        if (this.intervalId) {
            clearInterval(this.intervalId);
        }
        this.intervalId = setInterval(() => this.checkForUpdates(), this.checkInterval);
    }

    stopPeriodicCheck() {
        if (this.intervalId) {
            clearInterval(this.intervalId);
            this.intervalId = null;
        }
    }

    updatePreview() {
        // Trigger the existing preview update function if it exists
        if (typeof updatePreview === 'function') {
            updatePreview();
        }
    }

    showDraftStatus(message, type = 'info') {
        // Create or update status message
        let statusEl = document.getElementById('draft-status');
        if (!statusEl) {
            statusEl = document.createElement('div');
            statusEl.id = 'draft-status';
            statusEl.className = 'draft-status';
            
            // Insert after breadcrumb
            const breadcrumb = document.querySelector('.breadcrumb');
            if (breadcrumb) {
                breadcrumb.insertAdjacentElement('afterend', statusEl);
            }
        }
        
        statusEl.textContent = message;
        statusEl.className = `draft-status ${type}`;
        
        // Auto-hide after 3 seconds for info messages
        if (type === 'info') {
            setTimeout(() => {
                if (statusEl && statusEl.textContent === message) {
                    statusEl.style.opacity = '0';
                    setTimeout(() => statusEl.remove(), 300);
                }
            }, 3000);
        }
    }
}

// Initialize editor storage when DOM is loaded
let editorStorage = null;

function initializeEditorStorage() {
    // Get file path from hidden input
    const pathInput = document.querySelector('input[name="path"]');
    if (pathInput) {
        const filePath = pathInput.value;
        editorStorage = new EditorStorage(filePath);
        
        // Set up auto-save on text change
        const textarea = document.querySelector('textarea[name="content"]');
        if (textarea) {
            let saveTimeout;
            textarea.addEventListener('input', () => {
                clearTimeout(saveTimeout);
                saveTimeout = setTimeout(() => {
                    editorStorage.saveDraft();
                }, 1000); // Save draft 1 second after stopping typing
            });
        }
        
        // Handle form submission
        const form = document.querySelector('form[action="/save"]');
        if (form) {
            form.addEventListener('submit', () => {
                // Clear draft when successfully saving
                setTimeout(() => {
                    editorStorage.clearDraft();
                }, 100);
            });
        }
        
        // Handle delete form submission
        const deleteForm = document.getElementById('deleteForm');
        if (deleteForm) {
            deleteForm.addEventListener('submit', () => {
                editorStorage.clearDraft();
            });
        }
    }
}

// Clean up when page unloads
globalThis.addEventListener('beforeunload', () => {
    if (editorStorage) {
        editorStorage.stopPeriodicCheck();
    }
});

// Initialize when DOM is ready
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initializeEditorStorage);
} else {
    initializeEditorStorage();
}