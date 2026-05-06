import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import rehypeRaw from 'rehype-raw';
import { invoke } from '@tauri-apps/api/core';
import 'highlight.js/styles/github-dark.css';

interface MarkdownProps {
  content: string;
  className?: string;
  allowRawHtml?: boolean;
}

export function Markdown({ content, className = '', allowRawHtml = false }: MarkdownProps) {
  const rehypePlugins = [rehypeHighlight];
  if (allowRawHtml) {
    rehypePlugins.push(rehypeRaw as any);
  }

  return (
    <div className={className}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={rehypePlugins}
        components={{
          a: ({ href, children, ...props }) => {
            const handleClick = (e: React.MouseEvent) => {
              if (!href) return;
              
              // Check if it's an external link or a specific protocol
              // We treat anything that looks like an absolute URL or a specific protocol as external
              const isExternal = /^(https?|obsidian|mailto|tel|doxus):/.test(href) || 
                                (href.startsWith('www.') && !href.startsWith('/'));
              
              if (isExternal) {
                e.preventDefault();
                // For www. links without protocol, add https://
                const finalUrl = href.startsWith('www.') ? `https://${href}` : href;
                
                invoke('plugin_open_url', { url: finalUrl }).catch((err) => {
                  console.error('Failed to open URL:', err);
                  window.open(finalUrl, '_blank', 'noopener,noreferrer');
                });
              }
            };

            return (
              <a
                href={href}
                onClick={handleClick}
                target="_blank"
                rel="noopener noreferrer"
                {...props}
              >
                {children}
              </a>
            );
          },
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}
