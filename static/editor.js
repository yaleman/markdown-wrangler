/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

const textarea = document.querySelector('textarea');
const preview = document.getElementById('preview');

function escapeHtml(content) {
    return content
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;');
}

function normalizeCodeLanguage(language) {
    if (!language) {
        return 'text';
    }
    const normalizedLanguage = language.trim().toLowerCase();
    if (/^[a-z0-9_-]+$/i.test(normalizedLanguage)) {
        return normalizedLanguage;
    }
    return 'text';
}

function extractCodeBlocks(content) {
    const codeBlocks = [];
    const withPlaceholders = content.replace(
        /```([a-z0-9_-]+)?[ \t]*\r?\n([\s\S]*?)```/gim,
        (_, language, codeContent) => {
            const normalizedLanguage = normalizeCodeLanguage(language);
            const normalizedCode = codeContent.replace(/\r?\n$/, '');
            const index =
                codeBlocks.push(
                    `<pre class="language-${normalizedLanguage}"><code class="language-${normalizedLanguage}">${normalizedCode}</code></pre>`
                ) - 1;
            return `\n@@CODE_BLOCK_${index}@@\n`;
        }
    );

    return { withPlaceholders, codeBlocks };
}

function restoreCodeBlocks(content, codeBlocks) {
    return content.replace(/@@CODE_BLOCK_(\d+)@@/g, (_, index) => {
        const codeBlock = codeBlocks[Number(index)];
        return codeBlock || '';
    });
}

function extractCodeSpans(content) {
    const codeSpans = [];
    const withPlaceholders = content.replace(/`([^`\n]+)`/g, (_, codeContent) => {
        const index = codeSpans.push(`<code>${codeContent}</code>`) - 1;
        return `@@CODE_SPAN_${index}@@`;
    });

    return { withPlaceholders, codeSpans };
}

function restoreCodeSpans(content, codeSpans) {
    return content.replace(/@@CODE_SPAN_(\d+)@@/g, (_, index) => {
        const codeSpan = codeSpans[Number(index)];
        return codeSpan || '';
    });
}

// Function to strip frontmatter from markdown content
function stripFrontmatter(content) {
    // Handle YAML frontmatter (--- ... ---)
    const yamlFrontmatterRegex = /^---\s*\n([\s\S]*?)\n---\s*\n/;

    // Handle JSON frontmatter ({ ... })
    const jsonFrontmatterRegex = /^\{\s*\n([\s\S]*?)\n\}\s*\n/;

    // Try YAML first, then JSON
    let cleanContent = content.replace(yamlFrontmatterRegex, '');
    if (cleanContent === content) {
        cleanContent = content.replace(jsonFrontmatterRegex, '');
    }

    return cleanContent;
}

// Function to process lists with proper wrapping
function processLists(content) {
    const lines = content.split('\n');
    const result = [];
    let currentList = null;
    let currentListType = null;

    for (let i = 0; i < lines.length; i++) {
        const line = lines[i];
        const unorderedMatch = line.match(/^(\s*)[-*+]\s+(.*)$/);
        const orderedMatch = line.match(/^(\s*)\d+\.\s+(.*)$/);

        if (unorderedMatch) {
            const text = unorderedMatch[2];

            if (currentListType !== 'ul' || currentList === null) {
                if (currentList !== null) {
                    result.push(`</${currentListType}>`);
                }
                result.push('<ul>');
                currentList = [];
                currentListType = 'ul';
            }
            result.push(`<li>${text}</li>`);
        } else if (orderedMatch) {
            const text = orderedMatch[2];

            if (currentListType !== 'ol' || currentList === null) {
                if (currentList !== null) {
                    result.push(`</${currentListType}>`);
                }
                result.push('<ol>');
                currentList = [];
                currentListType = 'ol';
            }
            result.push(`<li>${text}</li>`);
        } else {
            if (currentList !== null) {
                result.push(`</${currentListType}>`);
                currentList = null;
                currentListType = null;
            }
            result.push(line);
        }
    }

    if (currentList !== null) {
        result.push(`</${currentListType}>`);
    }

    return result.join('\n');
}

// Simple markdown preview (basic implementation)
function updatePreview() {
    let content = textarea.value;

    // Strip frontmatter before processing
    content = stripFrontmatter(content);

    // Escape raw HTML before markdown processing to avoid parsing input as tags.
    content = escapeHtml(content);

    const { withPlaceholders: withCodeBlockPlaceholders, codeBlocks } = extractCodeBlocks(content);

    // Process lists first (before other processing)
    content = processLists(withCodeBlockPlaceholders);

    const { withPlaceholders, codeSpans } = extractCodeSpans(content);

    // Basic markdown processing (inline elements first)
    content = withPlaceholders
        .replace(/~~(.*?)~~/gim, '<del>$1</del>')
        .replace(/\*\*(.*?)\*\*/gim, '<strong>$1</strong>')
        .replace(/\*(.*?)\*/gim, '<em>$1</em>')
        .replace(/\[([^\]]+)\]\(([^)]+)\)/gim, '<a href="$2">$1</a>')
        .replace(/&lt;(https?:\/\/[^\s<>]+)&gt;/gim, '<a href="$1">$1</a>');

    content = restoreCodeSpans(content, codeSpans);

    // Process paragraphs and block elements
    content = content
        .replace(/^### (.*$)/gim, '<h3>$1</h3>')
        .replace(/^## (.*$)/gim, '<h2>$1</h2>')
        .replace(/^# (.*$)/gim, '<h1>$1</h1>')
        // Split on double newlines to create paragraphs
        .split(/\n\s*\n/)
        .map((paragraph) => {
            paragraph = paragraph.trim();
            if (!paragraph) return '';
            // Don't wrap headers, lists, or already wrapped HTML in <p> tags
            if (
                paragraph.match(/^<(?:h[1-6]|ul|ol|li|pre)/) ||
                paragraph.match(/^@@CODE_BLOCK_\d+@@$/)
            ) {
                return paragraph;
            }
            return `<p>${paragraph.replace(/\n/g, ' ')}</p>`;
        })
        .join('\n');

    content = restoreCodeBlocks(content, codeBlocks);

    const previewContent = document.createElement('div');
    // nosemgrep: javascript.browser.security.insecure-document-method.insecure-document-method
    previewContent.innerHTML = content || '<em>Preview will appear here as you type...</em>';
    preview.replaceChildren(previewContent);

    if (window.Prism && content) {
        window.Prism.highlightAllUnder(previewContent);
    }
}

// Initialize editor functionality when DOM is loaded
if (textarea && preview) {
    textarea.addEventListener('input', updatePreview);
    updatePreview(); // Initial preview
}
