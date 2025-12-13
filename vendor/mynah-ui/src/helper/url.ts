export const getOrigin = (site: string): string => {
  let result = '';
  try {
    result = new URL(site).origin;
  } catch (err) {
    result = 'unknown';
  }
  return result;
};
