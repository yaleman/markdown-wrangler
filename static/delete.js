/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

function bindDeleteForms() {
	const forms = document.querySelectorAll('form#deleteForm');
	forms.forEach((form) => {
		form.addEventListener("submit", (event) => {
			const pathInput = form.querySelector('input[name="path"]');
			const filename = pathInput ? pathInput.value : "this file";
			const confirmed = confirm(
				`Are you sure you want to delete "${filename}"?\n\nThis action cannot be undone.`,
			);
			if (!confirmed) {
				event.preventDefault();
			}
		});
	});
}

if (document.readyState === "loading") {
	document.addEventListener("DOMContentLoaded", bindDeleteForms);
} else {
	bindDeleteForms();
}
