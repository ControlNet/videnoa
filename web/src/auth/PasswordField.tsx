import { Eye, EyeOff } from 'lucide-react';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Field, settingsInputClass } from '@/pages/settings/section';

/**
 * `dense` is the Settings-page form density. The sign-in panel is a different
 * surface with a different job and keeps the roomier default.
 */
export function PasswordField({ id, label, value, onChange, newPassword = false, dense = false, className }: {
  id: string; label: string; value: string; onChange: (value: string) => void; newPassword?: boolean; dense?: boolean; className?: string;
}) {
  const [visible, setVisible] = useState(false);
  const { t } = useTranslation('common');
  const input = <Input id={id} name={id} className={dense ? `${settingsInputClass} font-sans` : undefined} type={visible ? 'text' : 'password'} autoComplete={newPassword ? 'new-password' : 'current-password'} required value={value} onChange={(event) => { onChange(event.target.value); }} />;
  const toggle = <Button type="button" variant="outline" size="icon" className={dense ? 'size-8 shrink-0' : undefined} aria-label={t(visible ? 'auth.hidePassword' : 'auth.showPassword')} aria-pressed={visible} onClick={() => { setVisible(!visible); }}>{visible ? <EyeOff className={dense ? 'size-3.5' : 'h-4 w-4'} /> : <Eye className={dense ? 'size-3.5' : 'h-4 w-4'} />}</Button>;

  if (dense) {
    return <Field id={id} label={label} className={className}>
      <div className="flex gap-1.5">{input}{toggle}</div>
    </Field>;
  }

  return <div className="space-y-2">
    <label htmlFor={id} className="text-sm font-medium">{label}</label>
    <div className="flex gap-2">{input}{toggle}</div>
  </div>;
}
