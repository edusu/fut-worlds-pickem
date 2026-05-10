/**
 * Tiny presentational primitives shared by the three prediction tabs.
 * Kept in their own module so `shared.ts` can stay JSX-free.
 */

import type { ReactNode } from 'react';

export function Section({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle?: string;
  children: ReactNode;
}) {
  return (
    <section style={{ marginBottom: 24 }}>
      <h3 style={{ marginBottom: 4 }}>{title}</h3>
      {subtitle && <p style={{ fontSize: 12, opacity: 0.7, marginTop: 0 }}>{subtitle}</p>}
      {children}
    </section>
  );
}

interface MutationStatusProps {
  isSuccess: boolean;
  error: Error | null;
  successText?: string;
}

/**
 * Shared success/error footer for mutation buttons. Renders nothing while
 * the mutation is idle or pending — callers handle the pending label on
 * the button itself.
 */
export function MutationStatus({
  isSuccess,
  error,
  successText = 'Guardado.',
}: MutationStatusProps) {
  if (error) return <p style={{ color: 'tomato' }}>Error: {error.message}</p>;
  if (isSuccess) return <p style={{ color: 'seagreen' }}>{successText}</p>;
  return null;
}
