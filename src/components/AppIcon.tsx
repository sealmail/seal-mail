interface Props {
  className?: string;
  alt?: string;
}

export function AppIcon({ className, alt = "SealMail" }: Props) {
  return <img className={["app-icon", className].filter(Boolean).join(" ")} src="/icon.png" alt={alt} draggable={false} />;
}
