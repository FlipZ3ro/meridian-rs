import DashboardLayout from '../components/layout/DashboardLayout';
import { AuthGate } from '../components/auth/AuthGate';

export default function Page() {
  return (
    <AuthGate>
      <DashboardLayout />
    </AuthGate>
  );
}
