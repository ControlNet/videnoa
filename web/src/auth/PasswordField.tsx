import { Eye, EyeOff } from 'lucide-react';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';

export function PasswordField({ id, label, value, onChange, newPassword = false }: {
  id: string; label: string; value: string; onChange: (value: string) => void; newPassword?: boolean;
}) {
  const [visible, setVisible] = useState(false);
  const { t } = useTranslation('common');
  return <div className="space-y-2">
    <label htmlFor={id} className="text-sm font-medium">{label}</label>
    <div className="flex gap-2">
      <Input id={id} name={id} type={visible ? 'text' : 'password'} autoComplete={newPassword ? 'new-password' : 'current-password'} required value={value} onChange={(event) => onChange(event.target.value)} />
      <Button type="button" variant="outline" size="icon" aria-label={t(visible ? 'auth.hidePassword' : 'auth.showPassword')} aria-pressed={visible} onClick={() => setVisible(!visible)}>{visible ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}</Button>
    </div>
  </div>;
}
