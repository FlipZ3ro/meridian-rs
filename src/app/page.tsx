import MeridianTerminal from '../components/terminal/MeridianTerminal';
import { AuthGate } from '../components/auth/AuthGate';

export default function Page() {
  return (
    <AuthGate>
      <MeridianTerminal />
    </AuthGate>
  );
}
