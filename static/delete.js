/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

function confirmDelete(filename) {
    const confirmed = confirm(
        `Are you sure you want to delete "${filename}"?\n\nThis action cannot be undone.`
    );

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
