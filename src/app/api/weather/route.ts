import { NextResponse } from 'next/server';

const MSN_WEATHER_URL = 'https://www.msn.com/id-id/cuaca/prakiraan/in-Magelang%2C-Magelang-Tengah,Jawa-Tengah?loc=eyJsIjoiTWFnZWxhbmcsIE1hZ2VsYW5nIFRlbmdhaCIsInIiOiJKYXdhIFRlbmdhaCIsInIyIjoiS290YSBNYWdlbGFuZyIsImMiOiJJbmRvbmVzaWEiLCJpIjoiSUQiLCJnIjoiaWQtaWQiLCJ4IjoiMTEwLjIxNCIsInkiOiItNy40NyJ9&weadegreetype=C&ocid=winp2fptaskbar&cvid=e4621d29900b4c21eee42fb4bc747dc6&content=TeaserHumidity_wxnwtshmdt';

const fallbackWeather = {
  temperature: 21,
  condition: 'Cloudy',
  location: 'Magelang, Jawa Tengah',
  humidity: 86,
  wind: '2.9 km/h',
  pressure: '972 hPa',
  forecast: [
    { day: 'Sat', temp: 28 },
    { day: 'Sun', temp: 28 },
    { day: 'Mon', temp: 28 },
    { day: 'Tue', temp: 28 },
    { day: 'Wed', temp: 28 },
  ],
  source: MSN_WEATHER_URL,
};

const findNumber = (html: string, patterns: RegExp[]) => {
  for (const pattern of patterns) {
    const match = html.match(pattern);
    if (match?.[1]) return Number(match[1]);
  }

  return undefined;
};

const findText = (html: string, patterns: RegExp[]) => {
  for (const pattern of patterns) {
    const match = html.match(pattern);
    if (match?.[1]) return match[1].replace(/\\u002F/g, '/').replace(/&amp;/g, '&').trim();
  }

  return undefined;
};

export async function GET() {
  try {
    const response = await fetch(MSN_WEATHER_URL, {
      cache: 'no-store',
      headers: {
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126 Safari/537.36',
        Accept: 'text/html,application/xhtml+xml',
      },
    });

    if (!response.ok) return NextResponse.json(fallbackWeather);

    const html = await response.text();
    const temperature = findNumber(html, [/"temp"\s*:\s*(-?\d+)/i, /"temperature"\s*:\s*(-?\d+)/i, /(-?\d+)\s*°\s*C/i]);
    const humidity = findNumber(html, [/"humidity"\s*:\s*(\d+)/i, /Kelembapan[^\d]{0,40}(\d+)%/i, /Humidity[^\d]{0,40}(\d+)%/i]);
    const condition = findText(html, [/"caption"\s*:\s*"([^"]+)"/i, /"condition"\s*:\s*"([^"]+)"/i]);
    const windSpeed = findNumber(html, [/"windSpeed"\s*:\s*(\d+(?:\.\d+)?)/i, /Angin[^\d]{0,40}(\d+(?:\.\d+)?)/i, /Wind[^\d]{0,40}(\d+(?:\.\d+)?)/i]);
    const pressure = findNumber(html, [/"pressure"\s*:\s*(\d+)/i, /Tekanan[^\d]{0,40}(\d+)/i, /Pressure[^\d]{0,40}(\d+)/i]);

    return NextResponse.json({
      ...fallbackWeather,
      temperature: temperature ?? fallbackWeather.temperature,
      condition: condition ?? fallbackWeather.condition,
      humidity: humidity ?? fallbackWeather.humidity,
      wind: windSpeed ? `${windSpeed} km/h` : fallbackWeather.wind,
      pressure: pressure ? `${pressure} hPa` : fallbackWeather.pressure,
    });
  } catch {
    return NextResponse.json(fallbackWeather);
  }
}
