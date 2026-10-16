import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

/** Assistant answers as Markdown (GFM: tables, task lists, strikethrough). */
export function Markdown({ text }: { text: string }) {
  return (
    <div className="md break-words">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children }) => (
            <a href={href} target="_blank" rel="noreferrer" className="underline underline-offset-2">
              {children}
            </a>
          ),
          pre: ({ children }) => (
            <pre className="my-2 overflow-x-auto rounded-md bg-background/80 p-3 text-xs leading-relaxed">{children}</pre>
          ),
          code: ({ className, children, ...rest }) => {
            const block = typeof className === "string" && className.startsWith("language-");
            return block ? (
              <code className={className} {...rest}>
                {children}
              </code>
            ) : (
              <code className="rounded bg-background/80 px-1 py-0.5 text-[0.9em]" {...rest}>
                {children}
              </code>
            );
          },
          p: ({ children }) => <p className="my-1.5 first:mt-0 last:mb-0">{children}</p>,
          ul: ({ children }) => <ul className="my-1.5 list-disc pl-5">{children}</ul>,
          ol: ({ children }) => <ol className="my-1.5 list-decimal pl-5">{children}</ol>,
          li: ({ children }) => <li className="my-0.5">{children}</li>,
          h1: ({ children }) => <h3 className="mt-3 mb-1 text-base font-semibold">{children}</h3>,
          h2: ({ children }) => <h3 className="mt-3 mb-1 text-base font-semibold">{children}</h3>,
          h3: ({ children }) => <h4 className="mt-2 mb-1 font-semibold">{children}</h4>,
          blockquote: ({ children }) => (
            <blockquote className="my-2 border-l-2 border-muted-foreground/40 pl-3 text-muted-foreground">{children}</blockquote>
          ),
          table: ({ children }) => (
            <div className="my-2 overflow-x-auto">
              <table className="text-xs [&_td]:border [&_td]:px-2 [&_td]:py-1 [&_th]:border [&_th]:px-2 [&_th]:py-1 [&_th]:font-semibold">
                {children}
              </table>
            </div>
          ),
          hr: () => <hr className="my-3 border-muted-foreground/30" />,
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}
