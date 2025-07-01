const textarea = document.querySelector('textarea');
const preview = document.getElementById('preview');

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

    // Process lists first (before other processing)
    content = processLists(content);

    // Basic markdown processing (inline elements first)
    content = content
        .replace(/~~(.*?)~~/gim, '<del>$1</del>')
        .replace(/\*\*(.*?)\*\*/gim, '<strong>$1</strong>')
        .replace(/\*(.*?)\*/gim, '<em>$1</em>')
        .replace(/\[([^\]]+)\]\(([^)]+)\)/gim, '<a href="$2">$1</a>')
        .replace(/`([^`]+)`/gim, '<code>$1</code>');

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
            if (paragraph.match(/^<(?:h[1-6]|ul|ol|li)/)) {
                return paragraph;
            }
            return `<p>${paragraph.replace(/\n/g, ' ')}</p>`;
        })
        .join('\n');

    preview.innerHTML = content || '<p><em>Preview will appear here as you type...</em></p>';
}

// Initialize editor functionality when DOM is loaded
if (textarea && preview) {
    textarea.addEventListener('input', updatePreview);
    updatePreview(); // Initial preview
}
