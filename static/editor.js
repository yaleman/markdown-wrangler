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

// Simple markdown preview (basic implementation)
function updatePreview() {
    let content = textarea.value;
    
    // Strip frontmatter before processing
    content = stripFrontmatter(content);
    
    // Basic markdown processing
    content = content
        .replace(/^### (.*$)/gim, '<h3>$1</h3>')
        .replace(/^## (.*$)/gim, '<h2>$1</h2>')
        .replace(/^# (.*$)/gim, '<h1>$1</h1>')
        .replace(/\*\*(.*?)\*\*/gim, '<strong>$1</strong>')
        .replace(/\*(.*?)\*/gim, '<em>$1</em>')
        .replace(/\[([^\]]+)\]\(([^)]+)\)/gim, '<a href="$2">$1</a>')
        .replace(/`([^`]+)`/gim, '<code>$1</code>')
        .replace(/\n/gim, '<br>');
    
    preview.innerHTML = content || '<p><em>Preview will appear here as you type...</em></p>';
}

// Initialize editor functionality when DOM is loaded
if (textarea && preview) {
    textarea.addEventListener('input', updatePreview);
    updatePreview(); // Initial preview
}