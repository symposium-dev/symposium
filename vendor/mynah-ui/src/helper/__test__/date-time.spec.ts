import { getTimeDiff } from '../date-time';

describe('date-time', () => {
  describe('getTimeDiff', () => {
    it('minutes', () => {
      expect(getTimeDiff(0)).toEqual('1min');
      expect(getTimeDiff(60000)).toEqual('1min');
      expect(getTimeDiff(60000, { minutes: false })).toEqual('');
      expect(getTimeDiff(120000)).toEqual('2min');
      expect(getTimeDiff(180000)).toEqual('3min');
    });
    it('hours', () => {
      expect(getTimeDiff(3_600_000)).toEqual('1hr');
      expect(getTimeDiff(3_600_000, { hours: false })).toEqual('');
      expect(getTimeDiff(7_200_000)).toEqual('2hr');
    });
    it('days', () => {
      expect(getTimeDiff(86_400_000)).toEqual('1da');
      expect(getTimeDiff(86_400_000, { days: false })).toEqual('');
      expect(getTimeDiff(172_800_000)).toEqual('2da');
    });
    it('weeks', () => {
      expect(getTimeDiff(604_800_000)).toEqual('1we');
      expect(getTimeDiff(604_800_000, { weeks: false })).toEqual('');
      expect(getTimeDiff(1_209_600_000)).toEqual('2we');
    });
    it('months', () => {
      expect(getTimeDiff(2_592_000_000)).toEqual('1mo');
      expect(getTimeDiff(2_592_000_000, { months: false })).toEqual('');
      expect(getTimeDiff(5_184_000_000)).toEqual('2mo');
    });
    it('years', () => {
      expect(getTimeDiff(31_104_000_000)).toEqual('1yr');
      expect(getTimeDiff(31_104_000_000, { years: false })).toEqual('');
      expect(getTimeDiff(62_208_000_000)).toEqual('2yr');
    });
  });
});
