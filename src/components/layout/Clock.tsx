'use client';

import { useEffect, useState } from 'react';

const getClock = () => {
  const now = new Date();

  return {
    date: now.toLocaleDateString('en-US', { month: 'short', day: 'numeric' }),
    numericDate: now.toLocaleDateString('en-US', { month: 'numeric', day: 'numeric', year: 'numeric' }),
    time: now.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false }),
    timeWithPeriod: now.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: true }),
  };
};

const initialClock = {
  date: '--',
  numericDate: '--/--/----',
  time: '--:--',
  timeWithPeriod: '--:-- --',
};

export const Clock = ({ type }: { type: 'date' | 'numericDate' | 'time' | 'timeWithPeriod' }) => {
  const [clock, setClock] = useState(initialClock);

  useEffect(() => {
    setClock(getClock());
    const timer = window.setInterval(() => setClock(getClock()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  return <span>{clock[type]}</span>;
};
