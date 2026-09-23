import { useRef, useState, type FormEvent } from 'react';
import { Eye, EyeOff, LoaderCircle } from 'lucide-react';
import { login } from './api';

export function AuthScreen({ notice }: { notice: string }) {
  const [register, setRegister] = useState(false);
  const [visible, setVisible] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const pending = useRef(false);

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (pending.current) return;
    pending.current = true; setBusy(true); setError('');
    const values = new FormData(event.currentTarget);
    try {
      await login({ email: String(values.get('email')), password: String(values.get('password')) }, register);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not sign in. Please try again.');
    } finally { pending.current = false; setBusy(false); }
  };

  return <div className="auth-page">
    <header className="auth-header"><div className="auth-brand"><span className="brand-mark">F</span><strong>Forge</strong></div></header>
    <main className="auth-main"><section className="auth-content" aria-labelledby="auth-title">
      <h1 id="auth-title">{register ? 'Create your account' : 'Sign in to Forge'}</h1>
      <form className="auth-form" onSubmit={submit} aria-busy={busy}>
        <label htmlFor="auth-email">Email</label>
        <input id="auth-email" name="email" type="email" required autoFocus autoComplete="username" maxLength={254} disabled={busy} />
        <label htmlFor="auth-password">Password</label>
        <div className="auth-password"><input id="auth-password" name="password" type={visible ? 'text' : 'password'} required minLength={register ? 10 : 1} maxLength={256} autoComplete={register ? 'new-password' : 'current-password'} aria-describedby={register ? 'password-hint' : undefined} disabled={busy} /><button className="icon-button" type="button" title={visible ? 'Hide password' : 'Show password'} aria-label={visible ? 'Hide password' : 'Show password'} aria-pressed={visible} onClick={() => setVisible(!visible)}>{visible ? <EyeOff size={17} /> : <Eye size={17} />}</button></div>
        {register && <p id="password-hint" className="auth-hint">At least 10 characters.</p>}
        {(error || notice) && <p className="auth-notice" role="alert">{error || notice}</p>}
        <button type="submit" className="button primary-button auth-submit" disabled={busy}>{busy && <LoaderCircle size={17} className="spinning" />}{busy ? register ? 'Creating account...' : 'Signing in...' : register ? 'Create account' : 'Sign in'}</button>
      </form>
      <p className="auth-switcher">{register ? 'Already have an account?' : 'New to Forge?'}<button type="button" className="text-button" disabled={busy} onClick={() => { setRegister(!register); setVisible(false); setError(''); }}>{register ? 'Sign in' : 'Create an account'}</button></p>
    </section></main>
  </div>;
}
