function confirmDelete(filename) {
    const confirmed = confirm(`Are you sure you want to delete "${filename}"?\n\nThis action cannot be undone.`);
    
    if (confirmed) {
        // Submit the hidden delete form
        const deleteForm = document.getElementById('deleteForm');
        if (deleteForm) {
            deleteForm.submit();
        } else {
            console.error('Delete form not found');
            alert('Error: Could not find delete form. Please refresh the page and try again.');
        }
    }
}