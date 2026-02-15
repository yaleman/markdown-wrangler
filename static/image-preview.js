/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

function updateImageDimensions() {
    const img = document.getElementById('previewImage');
    const dimensionsSpan = document.getElementById('imageDimensions');
    if (img && dimensionsSpan) {
        dimensionsSpan.textContent = `${img.naturalWidth} × ${img.naturalHeight} pixels`;
    }
}

if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => {
        const img = document.getElementById('previewImage');
        if (img) {
            img.addEventListener('load', updateImageDimensions);
            if (img.complete) {
                updateImageDimensions();
            }
        }
    });
} else {
    const img = document.getElementById('previewImage');
    if (img) {
        img.addEventListener('load', updateImageDimensions);
        if (img.complete) {
            updateImageDimensions();
        }
    }
}
