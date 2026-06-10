import type { ReactNode } from 'react';

type GlassCardProps = {
  children: ReactNode;
  className?: string;
};

export const GlassCard = ({ children, className = '' }: GlassCardProps) => (
  <section className={`glass-card ${className}`.trim()}>{children}</section>
);
