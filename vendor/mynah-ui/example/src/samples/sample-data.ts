import {
    ChatItem,
    ChatItemContent,
    ChatItemType,
    DetailedList,
    DetailedListItemGroup,
    generateUID,
    MynahIcons,
    MynahUIDataModel,
    MynahUITabStoreTab,
    SourceLink,
    Status,
} from '@aws/mynah-ui';
import md0 from './sample-0.md';
import md1 from './sample-1.md';
import md2 from './sample-2.md';
import md3 from './sample-3.md';
import md4 from './sample-4.md';
import md5 from './sample-5.md';
import md6 from './sample-6.md';
import md7 from './sample-7.md';
import md8 from './sample-8.md';
import md9 from './sample-9.md';
import md10 from './sample-10.md';
import sampleList0 from './sample-list-0.md';
import sampleList1 from './sample-list-1.md';
import sampleList2 from './sample-list-2.md';
import sampleList3 from './sample-list-3.md';
import sampleList4 from './sample-list-4.md';
import SampleCode from './sample-code.md';
import SampleDiff from './sample-diff.md';
import SampleDiffApplied from './sample-diff-applied.md';
import SampleAllInOne from './sample-all-in-one.md';
import SampleTable from './sample-table.md';
import { Commands } from '../commands';

export const mynahUIQRImageBase64 =
    'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAANcAAAD6CAMAAAAFkSJvAAAAVFBMVEUAAAA4ODiCgoLw8PD29vb8/PwhISH///8ICAgUFBRZWVkrKyttbW20tLRERERlZWXIyMh3d3eTk5PS0tLi4uKdnZ3a2tpOTk7q6uqpqam+vr6Li4tMmuJgAAAeR0lEQVR42uxci3asKgwdBRR8i+Lz///zIlGJDM7o3NP23q6TrnZNKyLbQEh2Qh+Pv/JX/spf+VfC/8dyhkn085j/f2Uc68CHrZ9o/D+XQaVPE7Ae4l8gXe/gymj8K2Q4Agva+JdIJfAsVL8FVsxqhCv9NeqK4wYprPw9sGISWFz1L8LFkOUIfxOu5C+uX4GLDFdk28cJ2+6j9iM8gFDvx8Pt+NLajbm+9UVvDeYlrjANLsgIfksWJM26ISZB2aBu2jro89X0lkFSra1Kc3fa5zDwNgv6Cb9UWUD/NWw9843BvMFVXopozE5O+t1XaRfzWlgnky7dRAbNUNhWu2MqDBq6DCJCr2PeH5AYJWSXBiP/MC5wWszbGu1gQZPR8od5+Tgt4QO3rdAzYRO1w6KFcX9MX9XP4TIjho8538eCcMkdPTdzUj72cUvbynpxtN+v8+k/gctMtoQe/bSgM7M12aeo2u+HyYdagYy7F8S/RF9RfSold3BBr12YzEPcyHkO51lWjMikXhfOMCchjN3oixdJklWbndhb0XzOCcvLJAkcfYnsfDDiHq7gPB7rhE9fS496NkabWVDm99heQrgm3yVa8wdfwkAGxkNYXK8GE93ERe7j2pY9RHTe+FtZTXgDJ7gknXn4ajD3cPHPcA2WXxDdKS5kXZDk1mp+Ha4P9cUyZ/t5Co6i077N1hZ136Ev1jVHab24wrVZR+IuiYSRfttrB3w7i1UgiimmXXvwuVjb0TgPhHYcWNt0tbu+zGBa72A+0RdNRIRFzM+4NDLTSOgvM2KAuGqLhKm+tDaI0nrQOA36NET6JGEUla2+pC9nS0+mVwcXLZ3BhB/ri7qsVe3V18MxFkgVo21gtwRW7isJrbrM3BbafdmZhyeD+UhfblfhOS40lhd7fM92JzFzRmKeSIqvxXVRX110xCUaBxd6Bt+JFGAg0DPNeirofsmLi32jvljI8bB5Tc9xGd6rMrN0cZZ250n/3mg0Ub58fIHrO/UV00mGdQSOVxjmK6w2LEdinyH0pbDWP2QXD7JUA6tmucIiYzl3cSfnKiYqSRJhdT/9nL6Qk1FYVXX6pfPQ6isdjjRzZg2LUTiEZrT0rdUf0pcXl7JUq4vLOBUojgQTArEcf43r+/VlWvUWV2hNiIvL9Q8hrpx9thU6+EF9xYofu67v4gr3AA4J4gF+Rl8xlUEgCWumDuOSSxKxQLhIleembz7vScaZ28c3ZYqkhM5+Ul8LxRazWfBIWVwo5wu4mkJsOWA3KQyPZwwzfKtt+Ul92fAJ3HHrCz0QrtrZoh+XaNmf1ddx5QAufsSF0xyuzFdwfaW+6kv6yrnHAmBcznWef7e+Ei6wcM88JEZoTEPBhTSrgpSCL/oClfHUbEQ0QEUih4oRkRG9QLEMZCe3ES53MJ/rizVVVU3ou3NxdTXQzr2irBqbdbGTahyVGrXoH3m7pds8tSRiHseKsjFxmekibN156B/Mh/wGc76PuEj/MkPNMN3kXV/AguTCmZjG9adPPIBvMF/Cb6Dke0+vp0f5ERdzozSvv/H9fBQYB/YaF15f7haQeXFNX8JH0Xu4xHgjne0+eBK+ErTqCi/a3sQVzWcia5fHTvu+zCnN5VZhRXJZreobRtUgfYleS2Huivo+Udr7kjpUm8q+B9zB0iB1eOxXg7nJY9/JO0im8dCMP8Rs0AwJNzz2ottiS4KBvgpCKYW5U1KqWyvx4ImeZtomQrdM/3n+D+VTzEdY68pSU/NuUkBfJkqDoKvcCWLYl/PHnkuTX5dPuY3L9aPcOIU84WLHOMXFhezhT+IyDDUEyQsu7uKiXlyVF5f5azD8YVx1FKXvv0Y3/6VSEeTncSXWV+TgyjotkMqcu24wfRUQfIaXBqOu4BraK0JcXHHbDC/iZfoCF/Dcj/VjWhPdF7k9mD9Yl+LmU17wG+TFPHT9+39VNfjNuOh1XG99lx/CpayHFO523q8vmJIurp7+J3EtzJKARbcw1OBi+fUVS/EjuKqwtBKu2RI6Zmt6fzJBVFCWh1asHeeJkTGbu7hVM7hUEH9FS4tELIQN4GLTXNcm3Z/YqCwqn0XivC6banRpbu7imqLDe4xW28u3tMHkK8Rf6dzwWA79FH+VziLKz3kdI6hGCVV5QFTQ3cP1FBjVlnuWPo72qZU6j7+u4UIfR4dhdtOFN3AlPkMFbutyC0NxJUKYOEyuX1/uM9/ievGO6pvzULq49ho0iPpI4RtAeQWXmG7i8le9racabuKic7DSypF5QDFN1RC3ZVqMMIuaZLvED62mSQn0IttpytHREe366A5YU21LhjbTFCIwpi+UWolS/cQtkqzamITHcWW37TwFR2Vo0o2A1k4gG3bHhQ1tZzQRdEur6HHksKHjMT2cY+LjoD0fUgsegIUdMnE451QsfVV2a4umoV1NP1MpX9xBso5rdTA/3r+gkgbU4bANKP+F6m0eFhe692GjGcOe9uRY5Yb2L7RlR92Rz3B//zO43OS4HxfSl+ss4SjNy0cl57ig2ih3cNUf4zLG4iTpn+2vjCbOgg8tr82dUyO1xZX4TCUqeUC4Rn6s/YCXqu7gAm5522WmlO8EGD36OU2gV10TLyyHaYX0VS8c93jEhbNJolpo6yPzzguAke9PjJqd1wZcim70+dKKl8N1XG1YFEVQFKVal1ObK5WA5Sj67GClhzwf4ibrtXfVjkr1O2P9iHQnRWo9pFQptbLd9dqXlmh5C5FUIPk2yk73VWxPrKsVl+mmgMH1c6tbHQ8avsaFZpR0IhCYS85kZEOwm4HQ6wuBzWHHbC2uQfKSn7VzRG10+i3pPb+3sWvdzYj7c1aTrVuun5ImFhf14eJOFtpdu8jfcHFFzT1ck10T6EW+wKUex6Lr27j8BaazlylCbkt1D1f1Blfanecru9R7NPp8HoK+yGkhJpqHpP8cF8mlzLj7IttRSqhuD/o+q0yrRdZVC0rKpByHuMr6PtjI6r7X61v/MBM7neUmYMbNJShE2i6NB8UtFPfSwVa13dVbX+s5XilzcgnX0PPjBAHqKbUHERY73/arFebl0WfghX6xlMJmXFMtC3dNB3+eqFmu++38bsJMD/j3Nj10xZNLdl65E5/srdG+PDsHE5Sz9N7GKfx8X353fPApTrm0L2fOTaYWEhXQoYMj3AZ8yo1TnASON//F/bhu5wgv+VGlz3dlmaMvxY+mVrlHjYpjkEXO87C3cTFn7+N3z0ktme0EOIou2WIJkbdrmMGF4CZqGLp23m6IaljGVcCjmb7JV4qp7br18LutvymazkjrmMhhq+Ju98E89IcopDdx1VXV0D32q6py45oDxUx+XksbD6boGiqfqqrbTPlQtW/zlY9op7CjXHcG0Sffaq71Y5C9CNM0G+xgYAaptTDgFi7pY3Kf9g1pTUh+M7/s7stoGT+xBWbmh86WfWP/yq7gQrew/gquV3VE57iQRUBV25/hkucFPqMPMj5j13yur576cJXOGRFkUoyrGLWXcXX7CBLiYahXCqL14OIv/tvFO30B2/2EC0X4C+Eh8vhQK8xDdt2PamW2SK2GI3UcNnGrzKVModdETIwU6RtywqZwXuMrmtdZVmZZOAGPXXBodRBj13igW1UL8Z2VkVNolC7drm+rCetpH8zcxIPSl1hez92/yDss1HHqPeCE65ZNq2Znu1FpB8qnuFVw4KC49djIT5K+wUCZQSz58SDjXX6jcHkfHy7gJELLYz92Aou+wBU6NUn8TQgDliqzfNL8MS543aUv4YbqseEpiMd++PKVT+GrfMP3uv8eBPrqbQLtcz7KhEt8PGe7pS3H8OLy6ws48eoNLvd1rrgcUv0tLtZM01Lhd3hLQ5YG2i3qFo56OjoVZP6HtivRchQEglE88MT7yv//5woN0hxGTWbdffMyoyFWQOiurm6CIFz0qMjjq/4ie/O8GcJT7CUnXr2sEPoRRy+NGxm2BrJTA0GqT6wwrJIPdQ9COSkVb+P6ZUnjtzylTLI1oEUXp9GC2VMvLtxfQprNbc+6SWNCcNyh4+w0Wv7z/VfjLsZCWYaKVLesyFNcmhpxlln9bMOSLSYHdNVSWPEUb38hX24mVjxltcI4pVeXp0epy2Kd61J8ISeHRQk1C2Lnw/qfLy/bjbJm7+GyNdHZfT5qPLcPUasf8ny9uDDbPX+Pa/2ejxo0v/GOCBH/IyCNCVMZCwzrbZSaWinw7Odr4PQEZ7vrSbLdiN9Q75U6Pf5xb8x7ohk0JtFo47JJ9Y981DiCeFLwxeqYyyomm6CaFQm1qatmeU3mmzfofl5Q3Ktgu9tBMtSC21KN12A/8k+ccQiwnllHpGGW17VN4QlSvXnKH77oC8cMYJ2IbfZULTTUjS/b4XxRfQTM6Va/k77Ocwb0sltlZ1IBg/S9zfe+zke7Xw/gxQUT6Poy2W6/40K9FDc793HCb/jex7hKLSi2BYs/4EIlR5xry99wZdU5LjvSb3NGgMs3Dj8eMA7jC1z37EM+ObQo10JNHHkV68CUOFodjToOVqXRNk7pEqrJBOJfkBjcs1pWGkLxL+8RgGHDg1xErg74Zupau2r3+ovLqQ0vYZBzsZow1sQMSnZG1kwsVoNkVQFPGa9UosI4Sg31pZlxo4+ol6ptFaqE/jpuhkThw3Ho5bExbpupGH0sCHpX5VW9tda67Ff8l5Y7ZPM2D8ahl8f2m2h+XKE1927U5+M8xpVbIovuYX9ZqgPaev3IK1yZlaTsGHJPcUH/oJvZsm/4eZgyNI+tJMRLzBlqI+/zbQqH5R1vUlO8v2EtaLJ7M6CRUcXUnKss2TTobXJ9amFZNqPvIQZ/6ll/ZesGh+KxBVnN/+f7VNfIk5Cmy3ltTJG3kosuBBct5NXgfcLvslQbuqqAkiPGxySJ41dGfWNG/ncnd82e4XL4Rh3kR5VrNupzaQw9AFWtdyaP46zLoVepBt1xrmN+oCPy4sKk7uTDNdsRcX0qWCznxGtvQBJlfYe3cXC1X+NC+ih0ChmBgaWGQHqA2sqHRUWHbKmAY1R8kAqgRaf7A1yYOo5Laty8Zxy+ZMqU5uepMIvApLJ5bG+qpcuLYjLsNi/KhL3CWoPH3udqbjExxqnjLeQ0N2eoyVoy4J6T/Q9tKX6I6TLLpVq65VftZxKptC5Xwmno3R/rGJuVoLvd5LfVKKU1UNzwiY5P3IRAtVck2u7y2Hr9QnEHxDSB5l0z1M4zYRMPKIYLvcq1pEFv8b2X6xfmIzLp79Hwvn6e+eJECJfDUF/hmhITF6rb8x0uJN56ECfy8lEYV2IqCS9xIR/VrrP0HS40Bz3gozTphPSMCBcKfNzDhXSVdl0sVC/lO1z1ff4w7mqIWte7J9VLewM/XxPbLQd6xLq3QMe66Skuyi2HcNlNDy7oTgrOiTcNqLbLxnMAWS3jlUST6sP+iUd0fbddSG/Vejv3l6NByAwI1xNI+3BAuNKYTFOuhdewxnBtQpOc95fIxItXkdUXDUsaCfNOylO0E3v8oCgOu9VZITUTewNKDTFNQ8Q58aSN7uEyTejQ7i+d4oGoJIfJtftrPMYdLLOVyY2ccFLlsfiClYK0cjqeYsTBb+CC1V8M4TNcrzNcSe/zK1Fdx/AWb1NqsqWzvo3v41/IRYWp1VZP2DeAjMCa+PoLqRTbp7jeFo8dPtfPR3BA+P/Q6WkBXRRjtltcBM7HUTdV8Btmf/FMcpCwVPvLqHyMizdw8NjiZiLJgtzCFW/lPHO94zxn2uqMp1EJHkk3syZdqk4fhz5gGuEPfbq8FcFcqSoCKvNfFAGAxTo8moDVg/GXuaXfaHQDgbpqvxkhsbzvVzbZZ6G0YAaL/iJuK4wnuKqip8lq9+IpjbcgDjKcy4f2hngwbFxDcrqWOo5R58Nl6w+f4KJal1f9qD/04wpv4Xr78lNwBuFgsVaVhSv0MWBQEmH9kY9yxiEfYUlzJfhgx2RP5lNcyMMX2g+IunSWax1ZtwRzLVrK2tv+F+U1gQKhXsTeatRUXG9Y50rQ06ye7ZqiZlVXLdU6pT2vWgSWBG9V/g/qererplWRMftVJTy1b6DHgzqvUsIbGEIoeyTeWJeTkmrvv9/nsZlMdSFEzPO4v6Iyo7ziATmyzLJX5hTfJCx7FZW4al8NaLHxKlPwlNeLQVYLTjw71GIRZjyzim+ssOR7A1VqFJhC8Usi5nn6kLdxqGMxA6D+EwPfKXct1k54l7euiE1POHJAVFdk9Bvtv/FRAhe18x0cz+oDj80sPcDs1RG9fbjgY1rLX/J7M+2P/WXjEo+uE5JHV3WacGx9yni/X4nqilz1F3z19/XY4iuKai+uTW/soRhqY9ePUuNaWEbrPo33N0xzpjlxuKdlAfqpXRbzzt8JhcIlKdmf6GATDWjdD66S2AQ0a8lXuBZL3yuyblWIb5t4vUpcabLIUK9GzbbwSpTz7gA2Bg0dr7VK5t0bDEJiMtTDMbfyBvKilg/hxjOLcVXLZevv+ZVX49DRg4zeaGxvrkzOzmm2ufZhoRcLIDywQ+BJMU3/Dhd61uP5gt/oM9+kF16YNX6V4va9juhqPvygx8bW8nIRr+xu5euh5WT8UUd0q78QdezFRd/pBS6Lx/5EwgsPoh7+BJeeNxK9ywcgmBlT1HFUha2sj50fdT72VyXmnjGuZQxXeWoK4VqWiwaysg1X+RWSMTTIazIyWcnExkXzth2Hr/rLydlCDHVr8jae7UNMXHyLGBl38N4JmFRLbnb4Bz2b4Kbq4U9wNb4Q+odafojfqKjnWUd3IqW+9NTIWL2cwcM4UZSf5tg5c3V9jgvpAUZt+aE7QW4Iqju33cf1NF7ZnSaSOFKOD7jEVfD1wz4j9gyBFkC7nt6f44Idk8Lg2P5CvRDUsWCapyhtZkVeB03THxPnUYaMb1XCryoEJ76/YQ2SukmjSTSgrop4SB26/r033QtNOx1lglmPyO3+iNSrjLNHPAA98pPJNPWcoeY/evFzv81altDNe77zCa8RRV+8uK6sOxfxsnHwbZM24doErpkeWJawJV0msptFsMtKUqs+2T+m4+QxhRq96KV9iFPdfi98lxUoE/xV3Dy2JjdjNhGKV7R+gbEknicYraOW9bc6VzP3mUHVuZ7N+eNq6Yhu4Sr98TI9CXqV1ijl960Hstf/wmz35MNFLyhTB9etcdhdWGso+gSecG3l66E6gaElmmfaQLG5yT57iAvRd7f6a6iBhj49VrVqceIhjomNC/mVom4qPHVNAUHlGDWQjFgWlH0cfA4ucugJ7/Eb6bJ2/adKfXEzQpHZKR3eiu1+efsrHcauPyjucUqnVuzqJhto8MdEWyer14KDwKyatoWVCc4rFLzbH+sRnXjhOdYLefrLoSPq19kWWn7eJvW5Q8jv/7Ue0SfH6HX+fKU+yWXWfIMr+oCr/WNcjRdXd84h/YDL318QHw7/GJddnw3meWEs+TVogsCayd/1lxgcBql+Hida7xwwzKY2kAw1HO06NlFa5XV44iU2eVDu0021udjiSbXNBOW9SXF4NaVajw3HKuevbeAU96ETv1qX6Y1D74OwIH95N2uyfEnjD4QfiXmVKbMWCPRSwo29QwoAuBYGPLanv6LWJNUfxlNOjvGcB+iuZtHZd1XvDQZpXtTBJaJiLivyZ7hIfZr7fsI9F3dyQmhj8tgOru4bfuMBLqnU/bW/pov+cubD91e4svPDxjXMFOVyZrszgne51o8aUTthc+5Z1VTT+2WPiaHyhrmV71lSbPbzNe5NEVj+K7I8eb6Cpj87KmdfGL6jEmgMw6ri8+FcFAe1rRhq0tXwBz4fborSHtixq3Y8VdW68t2YGCq9ISKJFq5XctDlvJTlfG8+fLC/A/bl9Xd8qKQopoa1s4+IBGe/7FvrF26d2oPxL/bjcHBVWopzpce2GeoH9sZ3eW0P9k+xyTQU9MJ6bHKqhbT3y37SX9/wNs/7S8Q8oGqfFfOglh7b2Dew9YVaHvdX+6y/ojE0j8rGFW3vMOQ7OPa7K8bGJW067mSFeQ7yqSLPGTDS0F9ZnudK5r2MvIr2yFpeRVu0onhsGMhMf67YKDJRDcijeD3T26D9pKwt75x9sjSPLXVFXPcFIQl/HTPEMg5m1XPp+EY+2tIO08TxN7zNg/3a0FqKqClvfTan/oa9GwmaUrpzfgNxqu8vxuHz/Xxrb12RD/2lvU/MY1cH5P+C6/l+vm+tlmUPcE0mDtBL2SbwJ1yv2zrYZ/3FPZMskwXGFp7t8Zq5VbJm/CU7rJSG19VXuPju00wUgZV0eSH532o/NcXpGnBml0LbB9NriZrewqfhNPM7+vv+2v+FvZJ6Ay6qbsPgogXjDLgmlsD5gy0Y1EbiA0uSkqRkN9dE9dtc22+8nJMpTuC7aq99v9y3Dx89X9796E/q3Ebm8+TkV7bHmLbzAmD9ssfhU3v+yfPlx3VeD9ZfVwT5cqju91We70NcX/aXs0GBFxdK3LFDYRBJLON/7V2LdqMgEAUSKQ8FpXGt8v//uTNoDKLm0SZ7Nj3ck5PWmKiXARyHeWzwGkUZpSYKmps+/Qt5XfyxdyzQoCoWES8MeSsC5qdmfq6qPeUouazl4N07zjiGdmzpi9fJa1QaTmy0UC8UrzQffVCDJjdvxnxdhCLZgNmB4NO5VBE7ewmc+nbqtkVly+ARPqyteU+W19dF742g9vulnfXeSDteuXYkviATjZCfbdhZPXju+KrE1ii+xguv8SsJeVstIm+64ozXtRP9+wx5jXbsSyaU1Rq+2l+si7L7pP5R7Y043yg/2/fkdUufjzzVQjZI8/VgP4x4+cTafSUu+/u8Jnmxbok2ff76LKmCATwMX2ihtou70anq+6hheFI8xKCNZlzeR/9Df/j4rM7LboOieqw8IGEXJjCq4qWyn8oL42+K6HUsVs/LxQFm5C5khjoeFieq9KJ2iHDoSu1jk/elXvYBrXEtPxcEvnhaEzt7bUcZCH4qrzvtAM1+X0kNqe3mrY3NSnw08UT35ZNeKu0/ltddvOI8nJvm22gebzdVx0s9xHhliydRsy7hpb4lrwfsh+W/lteoOPuP7/A6HfZQp7z+mDHz2jKSIR1fbWSC26pHXzgYRKAWHUO97PP4KicPBBGHVRSfcEZ1Skf0Pbwk3YVeuX+GOiO1xX10emOnaT5cLpi58C+bVwd9eAxj09XCfHhwZj5NqOo2NV21TCkCM/DhY+hoeXqQ1yPrKalrW2QkVFv9MLp7L60FxdoUVV5ZtOdkI3Xys3mljtLBEam4xSuNd1hpAuWNVehH/bF/zCuthB3IFKm/aORpvW2svhIW2BLyUP2Ub/ZDs+Xmf6mEPapY4VuR1j/asSM1JakIeC18Lmj95f1xAX1zD1YnxGcm1anpbapqVNTluM2mAX5qlIuVyC+nmrjJj5WdzNQd/Kypr/kz/UmO9Tz/jeSIRXHE1zF6/sVPl9u3D3PcOMrOVx+5f705Mq8349WRzTjZt+cVTcw/Knj8nyGuyXglMPPtsMg0/Xs64jL2RA6/hdcyKFLQ0++gNaSGdDr8grnj2K9K4AlZ1of3ZnUY1JbnvZBG2fJtYRWVJCMjIyMjIyMjIyMjIyMjI+M+iMiQsJOjQO86jnJ97cibn/4ruwUdM2yEFEc+OullQ7q9qxfMXKHVqK2WcOLlkhpPFC6toSOBuEIFnzxNOrZqZznu0U7ERevCQcX8bphYc5Ddi3lx5plXIBZc5ldD6y0RHnNFiMYS5Tlpmfchs6ajKFXPYA+TxDoumXOheAdIxDhMSMG1U6RrqR0TUyjvYb/upywVeBBbig4OSn3zWl7CK17WGtoPXQykVzCIhGs4kOoc9BZNKs9Ny7FDiuCyA41gGTXKEdXqDisy61YSXnW9baxw8LtedgOlDj6ktKVEVxS+IWzTNErQVkhoIPz7Ul4STi8xw//Y4UM/FIyTriQaRlyjSQujquygAUKaDccqRShzQ6mIBEmaUVzQIpJxZUEQONgow4OI0rEBeMGG55gBznZCQuvgSaR/sbycIV214CUuvHCq6JETJRT3817jNCCr3teUGA7NDlLBZjnzIqyBKx55wU6UzchrbkcROvSrecH1OtdDL/EDwxHRWm90ZaXvuWwbVysz9CUOBtwLkm3YAJ2ybcwgCestg8uzODLNQJlVwJ1iLA2ruK46Db+smSwrTQc1tyPzdSkUG5h9cU+EwS7F5KgrjNJCai7QFZfDYMOcYB0V0OdGr11luMZIJ4GF5KkyMvRa9NmVkkvYFeZGDltaEq0ofDtszLc+rrBABp/9gl8G07Tq9l1V7DduPPNz27f/yUqBNvqJwqeGCpKRkZGRkZHx6/EXe9lNM3Onk3MAAAAASUVORK5CYII=';
export const mynahUIQRMarkdown = `![Mynah UI](${mynahUIQRImageBase64})`;

// react stateless function component example
export const exampleSources = [
    {
        url: 'https://github.com/aws/mynah-ui',
        title: 'MynahUI',
        body: '#### A Data & Event Drivent Chat Interface Library for Browsers and Webviews',
    },
    {
        url: 'https://github.com/aws/mynah-ui/blob/main/docs/STARTUP.md',
        title: 'MynahUI initial setup',
        body: `Simply install it from npm with your favorite package manager.
  \`\`\`
  npm install @aws/mynah-ui
  \`\`\`
  `,
    },
    {
        url: 'https://github.com/aws/mynah-ui/blob/main/docs/USAGE.md',
        title: 'How to use MynahUI',
        body: `To see how to configure statics for MynahUI please refer to **[Configuration](./CONFIG.md)** document.

  Lastly before you start reading here, you can find more details on the **[Data Model](./DATAMODEL.md)** document. That document also contains visuals related with each type of the chat message in detail.


  #### All publicly available functions
  \`\`\`typescript
  mynahUI.addChatItem(...);
  mynahUI.addToUserPrompt(...);
  mynahUI.getSelectedTabId();
  mynahUI.notify(...);
  mynahUI.updateLastChatAnswer(...);
  mynahUI.updateStore(...);
  \`\`\`
`,
    },
] as SourceLink[];

const progressiveFileListDefaults: ChatItem['fileList'] = {
    hideFileCount: true,
    fileTreeTitle: '',
    flatList: true,
    collapsed: true,
    rootFolderTitle: 'Reading',
    rootFolderLabel: 'portfolio',
    rootFolderStatusIcon: 'progress',
    rootFolderStatusIconForegroundStatus: 'info',
};

export const sampleProgressiveFileList: Partial<ChatItem>[] = [
    {
        body: 'Thinking...',
        type: ChatItemType.ANSWER,
        shimmer: true,
        canBeVoted: false,
    },
    {
        body: 'More thinking...',
    },
    {
        body: null,
        shimmer: false,
        header: {
            fileList: {
                fileTreeTitle: '',
                filePaths: ['portfolio'],
                details: {
                    portfolio: {
                        visibleName: 'Reading',
                        icon: 'folder',
                        labelIcon: 'progress',
                        labelIconForegroundStatus: 'info',
                        label: 'portfolio',
                    },
                },
            },
        },
    },
    {
        header: {
            fileList: {
                ...progressiveFileListDefaults,
                filePaths: ['/qdev-wbr/pytest.ini'],
                details: {
                    '/qdev-wbr/pytest.ini': {
                        icon: 'progress',
                        label: 'Working on',
                        visibleName: '/qdev-wbr/pytest.ini',
                        iconForegroundStatus: 'info',
                    },
                },
            },
        },
    },
    {
        header: {
            fileList: {
                ...progressiveFileListDefaults,
                filePaths: ['/qdev-wbr/pytest.ini', 'yy.ts'],
                details: {
                    '/qdev-wbr/pytest.ini': {
                        icon: 'ok-circled',
                        visibleName: '/qdev-wbr/pytest.ini',
                        label: 'Done',
                        iconForegroundStatus: 'success',
                    },
                    'yy.ts': {
                        icon: 'progress',
                        label: 'Working on',
                        iconForegroundStatus: 'info',
                    },
                },
            },
        },
    },
    {
        header: {
            fileList: {
                ...progressiveFileListDefaults,
                filePaths: ['/qdev-wbr/pytest.ini', 'yy.ts', 'zz.ts', 'tt.ts'],
                details: {
                    '/qdev-wbr/pytest.ini': {
                        icon: 'ok-circled',
                        visibleName: '/qdev-wbr/pytest.ini',
                        label: 'Done',
                        iconForegroundStatus: 'success',
                    },
                    'yy.ts': {
                        icon: 'ok-circled',
                        label: 'Done',
                        iconForegroundStatus: 'success',
                    },
                    'zz.ts': {
                        icon: 'progress',
                        label: 'Working on',
                        iconForegroundStatus: 'info',
                    },
                    'tt.ts': {
                        icon: 'file',
                        label: 'In queue',
                    },
                },
            },
        },
    },
    {
        header: {
            fileList: {
                ...progressiveFileListDefaults,
                filePaths: ['/qdev-wbr/pytest.ini', 'yy.ts', 'zz.ts', 'tt.ts'],
                details: {
                    '/qdev-wbr/pytest.ini': {
                        icon: 'ok-circled',
                        visibleName: '/qdev-wbr/pytest.ini',
                        label: 'Done',
                        iconForegroundStatus: 'success',
                    },
                    'yy.ts': {
                        icon: 'ok-circled',
                        label: 'Done',
                        iconForegroundStatus: 'success',
                    },
                    'zz.ts': {
                        icon: 'ok-circled',
                        label: 'Done',
                        iconForegroundStatus: 'success',
                    },
                    'tt.ts': {
                        icon: 'progress',
                        label: 'Working on',
                        iconForegroundStatus: 'info',
                    },
                },
            },
        },
    },
    {
        header: {
            fileList: {
                ...progressiveFileListDefaults,
                filePaths: ['/qdev-wbr/pytest.ini', 'yy.ts', 'zz.ts', 'tt.ts'],
                rootFolderTitle: 'portfolio',
                rootFolderLabel: undefined,
                rootFolderStatusIconForegroundStatus: undefined,
                rootFolderStatusIcon: undefined,
                details: {
                    '/qdev-wbr/pytest.ini': {
                        icon: 'ok-circled',
                        label: 'Done',
                        visibleName: '/qdev-wbr/pytest.ini',
                        iconForegroundStatus: 'success',
                    },
                    'yy.ts': {
                        icon: 'ok-circled',
                        label: 'Done',
                        iconForegroundStatus: 'success',
                    },
                    'zz.ts': {
                        icon: 'ok-circled',
                        label: 'Done',
                        iconForegroundStatus: 'success',
                    },
                    'tt.ts': {
                        icon: 'cancel-circle',
                        label: 'Failed reading',
                        status: 'error',
                        iconForegroundStatus: 'error',
                    },
                },
            },
        },
    },
];

export const sampleMarkdownList: Partial<ChatItem>[] = [
    { autoCollapse: true, body: `${sampleList0 as string}` },
    { body: `${sampleList1 as string}` },
    { body: `${sampleList2 as string}` },
    { body: `${sampleList3 as string}` },
    { body: `${sampleList4 as string}` },
];

export const sampleAllInOneList: Partial<ChatItem>[] = [{ body: `${SampleAllInOne as string}` }];

export const sampleTableList: Partial<ChatItem>[] = [{ body: `${SampleTable as string}` }];

export const exampleStreamParts: Partial<ChatItem>[] = [
    { body: `${md0 as string}`, canBeVoted: false },
    { body: `${md1 as string}` },
    { body: `${md2 as string}` },
    { body: `${md3 as string}` },
    { body: `${md4 as string}` },
    { body: `${md5 as string}` },
    { body: `${md6 as string}` },
    { body: `${md7 as string}` },
    { body: `${md8 as string}` },
    { body: `${md9 as string}` },
    { body: `${md10 as string}` },
    {
        relatedContent: {
            content: exampleSources,
            title: 'Sources',
        },
        codeReference: [
            {
                recommendationContentSpan: {
                    start: 952,
                    end: 967,
                },
                information: 'Say Hello to **`MynahUI`**.',
            },
            {
                recommendationContentSpan: {
                    start: 1034,
                    end: 1409,
                },
                information:
                    'Reference code *under the Apache License 2.0 license* from repository **`@aws/mynah-ui`**.',
            },
        ],
    },
];

export const exampleCodeBlockToInsert = SampleCode;
export const exampleCodeDiff = SampleDiff;
export const exampleCodeDiffApplied = SampleDiffApplied;

export const tabbedData: ChatItemContent['tabbedContent'] = [
    {
        label: 'Examples',
        value: 'examples',
        icon: MynahIcons.PLAY,
        content: {
            body: `**Here are some examples you can find:**`,
            customRenderer: `<img aspect-ratio src="https://d1.awsstatic.com/logos/aws-logo-lockups/poweredbyaws/PB_AWS_logo_RGB_REV_SQ.8c88ac215fe4e441dc42865dd6962ed4f444a90d.png" alt="Powered by AWS">`,
        },
    },
];

export const exploreTabData: MynahUIDataModel = {
    tabBackground: false,
    compactMode: false,
    tabTitle: 'Explore Agents',
    promptInputVisible: false,
    tabHeaderDetails: {
        icon: MynahIcons.ASTERISK,
        title: 'Amazon Q Developer Agents',
        description: 'Software development',
    },
    chatItems: [
        {
            type: ChatItemType.ANSWER,
            hoverEffect: true,
            fullWidth: true,
            body: `### Feature development
Generate code across files with a task description.
`,
            icon: MynahIcons.CODE_BLOCK,
            footer: {
                tabbedContent: tabbedData,
            },
            buttons: [
                {
                    status: 'clear',
                    id: 'user-guide-dev',
                    disabled: false,
                    text: 'Read user guide',
                },
                {
                    status: 'main',
                    fillState: 'hover',
                    flash: 'once',
                    icon: MynahIcons.RIGHT_OPEN,
                    id: 'quick-start-dev',
                    text: `Quick start with **/dev**`,
                },
            ],
        },
        {
            type: ChatItemType.ANSWER,
            hoverEffect: true,
            fullWidth: true,
            body: `### Write
Automatically write code and commit it.
`,
            icon: MynahIcons.BUG,
            footer: {
                tabbedContent: tabbedData,
            },
            buttons: [
                {
                    status: 'clear',
                    id: 'user-guide-write',
                    disabled: false,
                    text: 'Read user guide',
                },
                {
                    disabled: false,
                    icon: MynahIcons.RIGHT_OPEN,
                    status: 'main',
                    fillState: 'hover',
                    flash: 'once',
                    id: 'quick-start-write',
                    text: `Quick start with **/write**`,
                },
            ],
        },
        {
            type: ChatItemType.ANSWER,
            hoverEffect: true,
            fullWidth: true,
            body: `### Generation
Generate code for selected codebase (supports python & java).
`,
            icon: MynahIcons.CHECK_LIST,
            footer: {
                tabbedContent: tabbedData,
            },
            buttons: [
                {
                    status: 'clear',
                    id: 'user-guide-generate',
                    disabled: false,
                    text: 'Read user guide',
                },
                {
                    disabled: false,
                    icon: MynahIcons.RIGHT_OPEN,
                    status: 'main',
                    fillState: 'hover',
                    flash: 'once',
                    id: 'quick-start-generate',
                    text: `Quick start with **/generate**`,
                },
            ],
        },
        {
            type: ChatItemType.ANSWER,
            hoverEffect: true,
            fullWidth: true,
            body: `### Transform
Transform your java project from an old version to a new one.
`,
            icon: MynahIcons.TRANSFORM,
            footer: {
                tabbedContent: tabbedData,
            },
            buttons: [
                {
                    status: 'clear',
                    id: 'user-guide-transform',
                    disabled: false,
                    text: 'Read user guide',
                },
                {
                    disabled: false,
                    icon: MynahIcons.RIGHT_OPEN,
                    status: 'main',
                    fillState: 'hover',
                    flash: 'once',
                    id: 'quick-start-transform',
                    text: `Quick start with **/transform**`,
                },
            ],
        },
    ],
};

export const accountDetailsTabData: MynahUIDataModel = {
    tabBackground: false,
    compactMode: false,
    tabTitle: 'Account Details',
    promptInputVisible: false,
    tabHeaderDetails: {
        title: `Account details`,
    },
    chatItems: [
        {
            type: ChatItemType.ANSWER,
            body: `### Subscription
Free Tier
`,
            buttons: [
                {
                    status: 'primary',
                    id: 'upgrade-subscription',
                    text: `Upgrade`,
                },
            ],
        },
        {
            type: ChatItemType.ANSWER,
            body: `### Usage
591/1000 queries used
$0.00 incurred in overages
Limits reset on 8/1/2025 at 12:00:00 GMT
`,
        },
    ],
};

export const qAgentQuickActions: MynahUIDataModel['quickActionCommands'] = [
    {
        commands: [
            {
                command: '/dev',
                icon: MynahIcons.CODE_BLOCK,
                description: 'Generate code to make a change in your project',
                placeholder: 'Describe your task or issue in as much detail as possible',
            },
            {
                command: '/doc',
                icon: MynahIcons.DOC,
                description: 'Generate documentation',
                placeholder: 'Type your question',
            },
            {
                command: '/test',
                icon: MynahIcons.CHECK_LIST,
                description: 'Generate unit tests for selected code',
                placeholder: 'Specify a function(s) in the current file (optional)',
            },
            {
                command: '/review',
                icon: MynahIcons.BUG,
                description: '... and fix code issues before committing',
                placeholder: 'Type your question',
            },
            {
                command: '/transform',
                icon: MynahIcons.TRANSFORM,
                description: 'Transform your Java project',
                placeholder: 'Type your question',
            },
        ],
    },
];

export const welcomeScreenTabData: MynahUITabStoreTab = {
    isSelected: true,
    store: {
        quickActionCommands: qAgentQuickActions,
        quickActionCommandsHeader: {
            status: 'warning',
            icon: MynahIcons.INFO,
            title: 'Q Developer agentic capabilities',
            description: 'You can now ask Q directly in the chat to generate code, documentation, and unit tests. You don\'t need to explicitly use /dev, /test, or /doc',
        },
        tabTitle: 'Welcome to Q',
        tabBackground: true,
        chatItems: [
            {
                type: ChatItemType.ANSWER,
                icon: MynahIcons.ASTERISK,
                messageId: 'new-welcome-card',
                body: `#### Work on a task with Q Developer Agents
_Generate code, scan for issues, and more._`,
                buttons: [
                    {
                        id: 'explore',
                        disabled: false,
                        text: 'Explore',
                    },
                    {
                        id: 'quick-start',
                        text: 'Quick start',
                        disabled: false,
                        status: 'main',
                    },
                ],
            },
        ],
        promptInputLabel: 'Or, start a chat',
        promptInputPlaceholder: 'Type your question',
        compactMode: true,
        tabHeaderDetails: {
            title: "Hi, I'm Amazon Q.",
            description: 'Where would you like to start?',
            icon: MynahIcons.Q,
        },
    },
};

export const exampleRichFollowups: ChatItem = {
    type: ChatItemType.SYSTEM_PROMPT,
    messageId: new Date().getTime().toString(),
    followUp: {
        text: 'Rich followups',
        options: [
            {
                pillText: 'Accept',
                icon: MynahIcons.OK,
                description: 'You can accept by clicking this.',
                status: 'success',
            },
            {
                pillText: 'Reject',
                icon: MynahIcons.CANCEL,
                status: 'error',
            },
            {
                pillText: 'Retry',
                icon: MynahIcons.REFRESH,
                status: 'warning',
            },
            {
                pillText: 'Do nothing',
                icon: MynahIcons.BLOCK,
                status: 'info',
            },
        ],
    },
};

export const defaultFollowUps: ChatItem = {
    type: ChatItemType.ANSWER,
    messageId: generateUID(),
    followUp: {
        text: 'Example card types',
        options: [
            {
                command: Commands.REPLACE_FOLLOWUPS,
                pillText: 'Replace followups',
            },
            {
                command: Commands.STATUS_CARDS,
                pillText: 'Cards with status',
            },
            {
                command: Commands.HEADER_TYPES,
                pillText: 'Cards with headers',
            },
            {
                command: Commands.BORDERED_CARDS,
                pillText: 'Bordered cards',
            },
            {
                command: Commands.SUMMARY_CARD,
                pillText: 'Card with summary field',
            },
            {
                command: Commands.FORM_CARD,
                pillText: 'Form items',
            },
            {
                command: Commands.CARD_WITH_MARKDOWN_LIST,
                pillText: 'Markdown list',
            },
            {
                command: Commands.CARD_WITH_PROGRESSIVE_FILE_LIST,
                pillText: 'Progressive file list',
            },
            {
                command: Commands.CARD_WITH_ALL_MARKDOWN_TAGS,
                pillText: 'All markdown tags',
            },
            {
                command: Commands.CARD_RENDER_MARKDOWN_TABLE,
                pillText: 'Render markdown table',
            },
            {
                command: Commands.CARD_SNAPS_TO_TOP,
                pillText: 'Snaps to top',
            },
            {
                command: Commands.FILE_LIST_CARD,
                pillText: 'File list',
            },
            {
                command: Commands.PROGRESSIVE_CARD,
                pillText: 'Progressive',
            },
            {
                command: Commands.IMAGE_IN_CARD,
                pillText: 'Image inside',
            },
            {
                command: Commands.CUSTOM_RENDERER_CARDS,
                pillText: 'Custom renderers',
            },
            {
                pillText: 'Followups on right',
                command: Commands.FOLLOWUPS_AT_RIGHT,
            },
            {
                pillText: 'Information cards',
                command: Commands.INFORMATION_CARDS,
            },
            {
                pillText: 'Confirmation buttons',
                command: Commands.CONFIRMATION_BUTTONS,
            },
            {
                pillText: 'Buttons',
                command: Commands.BUTTONS,
            },
            {
                pillText: 'Sticky card',
                command: Commands.SHOW_STICKY_CARD,
            },
            {
                pillText: 'Some auto reply',
                prompt: 'Some random auto reply here.',
            },
        ],
    },
};

export const exampleFileListChatItem: ChatItem = {
    type: ChatItemType.ANSWER,
    fileList: {
        rootFolderTitle: 'Changes',
        filePaths: ['./package.json', './tsconfig.json', 'src/game.ts', 'tests/game.test.ts'],
        deletedFiles: [],
        actions: {
            './package.json': [
                {
                    icon: MynahIcons.CANCEL_CIRCLE,
                    status: 'error',
                    name: 'reject-change',
                    description: 'Reject change',
                },
            ],
            './tsconfig.json': [
                {
                    icon: MynahIcons.CANCEL_CIRCLE,
                    status: 'error',
                    name: 'reject-change',
                    description: 'Reject change',
                },
            ],
            'src/game.ts': [
                {
                    icon: MynahIcons.CANCEL_CIRCLE,
                    status: 'error',
                    name: 'reject-change',
                    description: 'Reject change',
                },
            ],
            'tests/game.test.ts': [
                {
                    icon: MynahIcons.CANCEL_CIRCLE,
                    status: 'error',
                    name: 'reject-change',
                    description: 'Reject change',
                },
            ],
        },
    },
};

export const exampleFileListChatItemForUpdate: Partial<ChatItem> = {
    type: ChatItemType.ANSWER,
    fileList: {
        rootFolderTitle: 'Changes',
        filePaths: ['src/index.ts'],
        deletedFiles: [],
        details: {
            'src/index.ts': {
                status: 'error',
                label: 'File rejected',
                icon: MynahIcons.CANCEL_CIRCLE,
                description: exampleCodeDiff,
            },
        },
        actions: {
            'src/index.ts': [
                {
                    icon: MynahIcons.REVERT,
                    name: 'revert-rejection',
                    description: 'Revert rejection',
                },
            ],
        },
    },
};

const itemId1 = generateUID();
const itemId2 = generateUID();

export const exampleFormChatItem: ChatItem = {
    type: ChatItemType.ANSWER,
    messageId: new Date().getTime().toString(),
    body: `Can you help us to improve our AI Assistant? Please fill the form below and hit **Submit** to send your feedback.

_To send the form, mandatory items should be filled._`,
    formItems: [
        {
            tooltip: 'Please select your expertise area',
            id: 'expertise-area',
            type: 'select',
            title: `Area of expertise`,
            icon: 'search',
            description: 'Select your area of expertise',
            options: [
                {
                    label: 'Frontend',
                    value: 'frontend',
                },
                {
                    label: 'Backend',
                    value: 'backend',
                },
                {
                    label: 'Data Science',
                    value: 'datascience',
                },
                {
                    label: 'Other',
                    value: 'other',
                },
            ],
        },
        {
            id: 'preferred-ide',
            type: 'radiogroup',
            title: `Preferred IDE`,
            options: [
                {
                    label: 'VSCode',
                    value: 'vscode',
                },
                {
                    label: 'JetBrains IntelliJ',
                    value: 'intellij',
                },
                {
                    label: 'Visual Studio',
                    value: 'visualstudio',
                },
            ],
        },
        {
            id: 'remote-ide',
            type: 'toggle',
            value: 'remote',
            title: `Environment`,
            options: [
                {
                    label: 'Remote',
                    value: 'remote',
                    icon: MynahIcons.STAR,
                },
                {
                    label: 'Local',
                    value: 'local',
                    icon: MynahIcons.SCROLL_DOWN,
                },
                {
                    label: 'Both',
                    value: 'both',
                    icon: MynahIcons.STACK,
                },
            ],
        },
        {
            id: 'is-online',
            type: 'checkbox',
            value: 'true',
            label: 'Yes',
            title: 'Are you working online?',
        },
        {
            id: 'is-monorepo',
            type: 'switch',
            label: 'Yes',
            icon: 'code-block',
            title: 'Are you working in a monorepo project?',
            tooltip: "If you're working more on monorepos, check this",
        },
        {
            id: 'working-hours',
            type: 'numericinput',
            title: `How many hours are you using an IDE weekly?`,
            placeholder: 'IDE working hours',
        },
        {
            id: generateUID(),
            type: 'list',
            title: 'Environment variables',
            items: [
                {
                    id: itemId1,
                    title: 'Name',
                    description: 'Variable key',
                    type: 'textinput',
                },
                {
                    id: itemId2,
                    title: 'Value',
                    description: 'Variable value',
                    type: 'textinput',
                },
            ],
            value: [
                {
                    persistent: true,
                    value: {
                        [itemId1]: 'some_env',
                        [itemId2]: 'AJSKJLE!@)(UD',
                    },
                },
                {
                    value: {
                        [itemId1]: 'some_other_env',
                        [itemId2]: '12kjlkj!dddaa',
                    },
                },
            ],
        },
        {
            id: 'email',
            type: 'email',
            mandatory: true,
            title: `Email`,
            description: 'Your email will be used to get back to you',
            placeholder: 'Email',
            checkModifierEnterKeyPress: true,
        },
        {
            id: 'name',
            type: 'textinput',
            mandatory: true,
            title: `Name (should contain "amazonq" and "aws" in the string)`,
            validationPatterns: {
                operator: 'and',
                patterns: [
                    {
                        pattern: 'amazonq',
                        errorMessage: 'Should contain amazonq!',
                    },
                    {
                        pattern: 'aws',
                        errorMessage: 'Should contain aws!',
                    },
                ],
            },
            placeholder: 'Name and Surname',
        },
        {
            id: 'ease-of-usage-rating',
            type: 'stars',
            mandatory: true,
            title: `How easy is it to use our AI assistant?`,
        },
        {
            id: 'accuracy-rating',
            type: 'stars',
            mandatory: true,
            title: `How accurate are the answers you get from our AI assistant?`,
        },
        {
            id: 'general-rating',
            type: 'stars',
            title: `How do feel about our AI assistant in general?`,
        },
        {
            id: 'skills',
            type: 'pillbox',
            title: 'Skills and Technologies',
            description: 'Add your programming languages and technologies (press Enter to add)',
            placeholder: 'Type a skill and press Enter',
            value: 'JavaScript,TypeScript,React',
        },
        {
            id: 'description',
            type: 'textarea',
            title: `Any other things you would like to share? (should contain one of "amazonq" or "aws", capital or not)`,
            validationPatterns: {
                operator: 'or',
                genericValidationErrorMessage: 'Should contain one of "amazonq" or "aws"',
                patterns: [
                    {
                        pattern: /amazonq/gi,
                    },
                    {
                        pattern: /aws/gi,
                    },
                ],
            },
            placeholder:
                'Write your feelings about our tool. If the form is fully filled and valid, Enter will submit the form',
        },
    ],
    buttons: [
        {
            id: 'submit',
            text: 'Submit',
            status: 'primary',
        },
        {
            id: 'cancel-feedback',
            text: 'Cancel',
            keepCardAfterClick: false,
            waitMandatoryFormItems: false,
        },
    ],
};

const checkIcons = {
    wait: '&#9744;',
    current: '&#9744;',
    done: '&#9745;',
};
export const exampleProgressCards: Partial<ChatItem>[] = [
    {
        body: `Hi there, we're currently working on your task. You can follow the steps below:

${checkIcons.wait} Reading your files in the project,

${checkIcons.wait} Analysing your project structure,

${checkIcons.wait} Finding weak points

${checkIcons.wait} Generating improvements

${checkIcons.wait} Creating a refactor plan

${checkIcons.wait} Showing the plan details

Once it is done, you'll be notified.`,
    },
    {
        body: `Hi there, we're currently working on your task. You can follow the steps below:

${checkIcons.current} Reading your files in the project,

${checkIcons.wait} Analysing your project structure,

${checkIcons.wait} Finding weak points

${checkIcons.wait} Generating improvements

${checkIcons.wait} Creating a refactor plan

${checkIcons.wait} Showing the plan details

Once it is done, you'll be notified.`,
    },
    {
        body: `Hi there, we're currently working on your task. You can follow the steps below:

${checkIcons.done} Reading your files in the project,

${checkIcons.current} Analysing your project structure,

${checkIcons.wait} Finding weak points

${checkIcons.wait} Generating improvements

${checkIcons.wait} Creating a refactor plan

${checkIcons.wait} Showing the plan details

Once it is done, you'll be notified.`,
    },
    {
        body: `Hi there, we're currently working on your task. You can follow the steps below:

${checkIcons.done} Reading your files in the project,

${checkIcons.done} Analysing your project structure,

${checkIcons.current} Finding weak points

${checkIcons.wait} Generating improvements

${checkIcons.wait} Creating a refactor plan

${checkIcons.wait} Showing the plan details

Once it is done, you'll be notified.`,
    },
    {
        body: `Hi there, we're currently working on your task. You can follow the steps below:

${checkIcons.done} Reading your files in the project,

${checkIcons.done} Analysing your project structure,

${checkIcons.done} Finding weak points

${checkIcons.current} Generating improvements

${checkIcons.wait} Creating a refactor plan

${checkIcons.wait} Showing the plan details

Once it is done, you'll be notified.`,
    },
    {
        body: `Hi there, we're currently working on your task. You can follow the steps below:

${checkIcons.done} Reading your files in the project,

${checkIcons.done} Analysing your project structure,

${checkIcons.done} Finding weak points

${checkIcons.done} Generating improvements

${checkIcons.current} Creating a refactor plan

${checkIcons.wait} Showing the plan details

Once it is done, you'll be notified.`,
    },
    {
        body: `Hi there, we're currently working on your task. You can follow the steps below:

${checkIcons.done} Reading your files in the project,

${checkIcons.done} Analysing your project structure,

${checkIcons.done} Finding weak points

${checkIcons.done} Generating improvements

${checkIcons.done} Creating a refactor plan

${checkIcons.current} Showing the plan details

Once it is done, you'll be notified.`,
    },
    {
        body: `Hi there, we're currently working on your task. You can follow the steps below:


${checkIcons.done} Reading your files in the project,

${checkIcons.done} Analysing your project structure,

${checkIcons.done} Finding weak points

${checkIcons.done} Generating improvements

${checkIcons.done} Creating a refactor plan

${checkIcons.done} Showing the plan details

Your Refactor analysis is ready! You can review it by opening the Markdown file: [Refactor_analysis_[id].pdf](#)
You can also ask me any follow-up questions that you have or adjust any part by generating a revised analysis.

`,
        fileList: {
            fileTreeTitle: '',
            filePaths: ['Refactor_analysis_[id].pdf'],
            details: {
                'Refactor_analysis_[id].pdf': {
                    icon: 'file',
                },
            },
        },
    },
];

export const exampleImageCard = (): ChatItem => {
    return {
        messageId: new Date().getTime().toString(),
        type: ChatItemType.ANSWER,
        body: `
### Image!
Here's a QR code for mynah-ui github link:

${mynahUIQRMarkdown}
`,
    };
};

export const exampleCustomRendererWithHTMLMarkup = (): ChatItem => {
    return {
        messageId: new Date().getTime().toString(),
        type: ChatItemType.ANSWER,
        canBeVoted: true,
        customRenderer: `
<h3>Custom renderer's with HTML markup string</h3>
<p>
Here you will find some custom html rendering examples which may not be available with markdown or pretty hard to generate.
</p>
<br />
<h3>Table (inside a blockqote)</h3>
<blockquote>
Most popular JS frameworks
<hr />
<table>
  <tbody>
    <tr>
      <th align="left">Name</td>
      <th align="right">Weekly Downloads</td>
    </tr>
    <tr>
      <td align="left">Vanilla</td>
      <td align="right"><strong>inf.</strong></td>
    </tr>
    <tr>
      <td align="left">React</td>
      <td align="right">24 <small>million</small></td>
    </tr>
    <tr>
      <td align="left">JQuery</td>
      <td align="right">10.6 <small>million</small></td>
    </tr>
    <tr>
      <td align="left">VUE</td>
      <td align="right">4.75 <small>million</small></td>
    </tr>
  </tbody>
</table>
</blockquote>

<br />
<hr />
<br />

<h3>Code block and Links</h3>

<pre class="language-typescript">
<code>import { MynahUI } from '@aws/mynah-ui';

const mynahUI = new MynahUI({});</code>
</pre>
<p>
You can find more information and references <strong><a href="https://github.com/aws/mynah-ui">HERE!</a></strong>.
</p>

<br />
<hr />
<br />

<h3>Embeds and Media elements</h3>

<h4>Iframe embed (Youtube example)</h4>
<iframe aspect-ratio="16:9" src="https://www.youtube.com/embed/bZsIPinetV4?si=k2Awd9in_wKgQC09&amp;start=65" title="YouTube video player" allow="picture-in-picture;" allowfullscreen></iframe>
<br />
<h4>Video element</h4>
<video aspect-ratio="21:9" controls="" poster="https://assets.aboutamazon.com/88/05/0feec6ff47bab443d2c82944bb09/amazon-logo.png">
  <source src="https://www.w3schools.com/tags/movie.mp4" type="video/mp4">
  <source src="https://www.w3schools.com/tags/movie.ogg" type="video/ogg">
  Your browser does not support the video tag.
</video>
<br />
<h4>Audio element</h4>
<audio controls>
  <source src="https://www.w3schools.com/tags/horse.ogg" type="audio/ogg">
  <source src="https://www.w3schools.com/tags/horse.mp3" type="audio/mpeg">
  Your browser does not support the audio tag.
</audio>
<br />
<h4>Image</h4>
<img aspect-ratio src="https://d1.awsstatic.com/logos/aws-logo-lockups/poweredbyaws/PB_AWS_logo_RGB_REV_SQ.8c88ac215fe4e441dc42865dd6962ed4f444a90d.png" alt="Powered by AWS">

<br />
<hr />
<br />

<p>There might be infinite number of possible examples with all supported tags and their attributes. It doesn't make so much sense to demonstrate all of them here.
You should go take a look to the <strong><a href="https://github.com/aws/mynah-ui/blob/main/docs/DATAMODEL.md">documentation</a></strong> for details and limitations.</p>
`,
    };
};

const attachmentIcon = `<svg width="30" height="30" viewBox="0 0 30 30" fill="none" xmlns="http://www.w3.org/2000/svg">
<rect width="30" height="30" rx="4" fill="#687078"/>
<path d="M19.6853 13.1011V13.1011C20.2085 12.195 19.898 11.0364 18.9919 10.5132L18.251 10.0854C17.0553 9.39509 15.5263 9.80478 14.8359 11.0005L11.0859 17.4957C10.3956 18.6914 10.8053 20.2204 12.001 20.9108V20.9108C13.1967 21.6011 14.7257 21.1914 15.4161 19.9957L17.7911 15.8821C18.1362 15.2842 17.9314 14.5197 17.3335 14.1746V14.1746C16.7357 13.8294 15.9712 14.0342 15.626 14.6321L14.0992 17.2765" stroke="white" stroke-width="1.5" stroke-linecap="round"/>
</svg>`;
export const exampleCustomRendererWithDomBuilderJson: ChatItem = {
    messageId: new Date().getTime().toString(),
    type: ChatItemType.ANSWER,
    canBeVoted: true,
    body: `Your Refactor analysis is ready! You can review it by opening the Markdown file: [file_name](#hello-pdf)
  You can also ask me any follow-up questions that you have or adjust any part by generating a revised analysis.`,
    customRenderer: [
        {
            type: 'blockquote',
            events: {
                click: (e: Event) => {
                    console.log('Hello!', e);
                },
            },

            children: [
                {
                    type: 'table',
                    children: [
                        {
                            type: 'tr',
                            children: [
                                {
                                    type: 'td',
                                    attributes: {
                                        style: 'min-width: 30px; width: 30px;',
                                    },
                                    children: [
                                        {
                                            type: 'img',
                                            attributes: {
                                                src: `data:image/svg+xml;base64,${window.btoa(attachmentIcon)}`,
                                            },
                                        },
                                    ],
                                },
                                {
                                    type: 'td',
                                    children: [
                                        {
                                            type: 'strong',
                                            children: ['Refactor_analysis_[id] .pdf'],
                                        },
                                    ],
                                },
                            ],
                        },
                    ],
                },
            ],
        },
    ],
};


export const exampleDownloadFile: ChatItem = {
    messageId: new Date().getTime().toString(),
    type: ChatItemType.ANSWER,
    canBeVoted: true,
    body: `Your Refactor analysis is ready! You can review it by opening the Markdown file: [file_name](#hello-pdf)
  You can also ask me any follow-up questions that you have or adjust any part by generating a revised analysis.`,
    fileList: {
        fileTreeTitle: 'Report',
        rootFolderTitle: '',
        filePaths: ['Refactor_analysis_[id] .pdf'],
    },
};

export const exampleInformationCard = (
    statusType: null | Status,
    statusBody: string | null,
    snap?: boolean,
): ChatItem => {
    return {
        messageId: generateUID(),
        snapToTop: snap === true,
        type: ChatItemType.ANSWER,
        informationCard: {
            title: 'Information card',
            description: 'With a description below the title.',
            icon: MynahIcons.BUG,
            content: {
                body: sampleList2 as string,
            },
            status:
                statusType === null || statusBody === null
                    ? {}
                    : {
                          status: statusType,
                          icon:
                              statusType === 'warning'
                                  ? MynahIcons.WARNING
                                  : statusType === 'error'
                                    ? MynahIcons.ERROR
                                    : MynahIcons.THUMBS_UP,
                          body: statusBody,
                      },
        },
    };
};

export const exampleBorderedCard = (): ChatItem => {
    return {
        messageId: generateUID(),
        type: ChatItemType.ANSWER,
        border: true,
        header: {
            padding: true,
            iconForegroundStatus: 'warning',
            icon: MynahIcons.INFO,
            body: '### /dev is going away soon!'
        },
        body: `With agentic coding, you can now ask me any coding question directly in the chat instead of using /dev.

You can ask me to do things like:
1. Create a project
2. Add a feature
3. Modify your files

Try it out by typing your request in the chat!`,
    };
};

export const exampleConfirmationButtons: ChatItem = {
    type: ChatItemType.ANSWER,
    messageId: new Date().getTime().toString(),
    body: 'This example shows some buttons with the `position` prop set to `outside`. Now we can use them to, for example, ask for confirmation! Does that make sense?',
    buttons: [
        {
            id: 'confirmation-buttons-cancel',
            text: `Cancel`,
            status: 'error',
            icon: MynahIcons.CANCEL_CIRCLE,
            position: 'outside',
        },
        {
            id: 'confirmation-buttons-confirm',
            text: `Confirm`,
            status: 'success',
            icon: MynahIcons.OK_CIRCLED,
            position: 'outside',
        },
    ],
};

export const exampleButtons: ChatItem = {
    type: ChatItemType.ANSWER,
    messageId: new Date().getTime().toString(),
    body: 'This is a card with actions inside!',
    buttons: [
        {
            text: 'With Icon',
            id: 'action-1',
            status: 'info',
            icon: MynahIcons.CHAT,
        },
        {
            text: 'Default',
            description: 'This has no status set!',
            id: 'action-2',
        },
        {
            text: 'Disabled',
            description: 'This is disabled for some reason!',
            id: 'action-3',
            disabled: true,
        },
        {
            text: 'Primary hover (with flash)',
            fillState: 'hover',
            id: 'action-3',
            flash: 'infinite',
            status: 'primary',
        },
        {
            text: 'Primary',
            description: 'This is colored!',
            id: 'action-3',
            status: 'primary',
        },
        {
            text: 'Main hover (with flash)',
            fillState: 'hover',
            id: 'action-3',
            flash: 'infinite',
            icon: MynahIcons.PROGRESS,
            status: 'main',
        },
        {
            text: 'Main',
            description: 'This is more colored!',
            id: 'action-3',
            status: 'main',
        },
        {
            text: 'Clear',
            description: 'This is clear!',
            id: 'action-3',
            status: 'clear',
        },
    ],
};

export const exampleStatusButtons: ChatItem = {
    type: ChatItemType.ANSWER,
    messageId: new Date().getTime().toString(),
    body: 'These are buttons with statuses',
    buttons: [
        {
            text: 'Proceed',
            id: 'proceed',
            icon: MynahIcons.OK,
            status: 'success',
            flash: 'infinite',
        },
        {
            text: 'Caution',
            id: 'caution',
            icon: MynahIcons.WARNING,
            status: 'warning',
        },
        {
            text: 'Cancel',
            id: 'cancel',
            icon: MynahIcons.CANCEL,
            status: 'error',
        },
        {
            text: 'Change Folder',
            id: 'change-folder',
            icon: MynahIcons.REFRESH,
            status: 'info',
        },
        {
            text: 'Change Folder',
            id: 'change-folder',
            icon: MynahIcons.REFRESH,
            status: 'info',
        },

        // External buttons
        {
            text: 'Proceed',
            id: 'proceed',
            fillState: 'hover',
            position: 'outside',
            icon: MynahIcons.OK,
            status: 'success',
            flash: 'infinite',
        },
        {
            text: 'Caution',
            fillState: 'hover',
            position: 'outside',
            id: 'caution',
            icon: MynahIcons.WARNING,
            status: 'warning',
        },
        {
            text: 'Cancel',
            fillState: 'hover',
            position: 'outside',
            id: 'cancel',
            icon: MynahIcons.CANCEL,
            status: 'error',
        },
        {
            text: 'Change Folder',
            fillState: 'hover',
            position: 'outside',
            id: 'change-folder',
            icon: MynahIcons.REFRESH,
            status: 'info',
        },
    ],
};

export const exampleVoteChatItem: ChatItem = {
    messageId: new Date().getTime().toString(),
    type: ChatItemType.ANSWER,
    canBeVoted: true,
    body: 'This chat item can be voted.',
};

export const sampleHeaderTypes: ChatItem[] = [
    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        padding: false,
        header: {
            icon: {
                name: 'javascript',
                base64Svg:
                    'data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMTIiIGhlaWdodD0iOCIgdmlld0JveD0iMCAwIDEyIDgiIGZpbGw9Im5vbmUiIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyI+CjxwYXRoIGQ9Ik0zLjEyIDAuMjRINC44MlY1QzQuODIgNi4wMjY2NyA0LjU4IDYuNzczMzMgNC4xIDcuMjRDMy42NiA3LjY2NjY3IDMgNy44OCAyLjEyIDcuODhDMS45MiA3Ljg4IDEuNyA3Ljg2NjY3IDEuNDYgNy44NEMxLjIyIDcuOCAxLjAyNjY3IDcuNzQ2NjcgMC44OCA3LjY4TDEuMDYgNi4zMkMxLjMxMzMzIDYuNDQgMS42MDY2NyA2LjUgMS45NCA2LjVDMi4zMTMzMyA2LjUgMi41OTMzMyA2LjQgMi43OCA2LjJDMy4wMDY2NyA1Ljk2IDMuMTIgNS41NiAzLjEyIDVWMC4yNFpNNi4zMiA2QzYuNTYgNi4xMzMzMyA2Ljg0IDYuMjQ2NjcgNy4xNiA2LjM0QzcuNTIgNi40NDY2NyA3Ljg2IDYuNSA4LjE4IDYuNUM4LjU4IDYuNSA4Ljg4IDYuNDMzMzMgOS4wOCA2LjNDOS4yOCA2LjE1MzMzIDkuMzggNS45NTMzMyA5LjM4IDUuN0M5LjM4IDUuNDQ2NjcgOS4yODY2NyA1LjI0NjY3IDkuMSA1LjFDOC45MTMzMyA0Ljk0IDguNTg2NjcgNC43OCA4LjEyIDQuNjJDNi43NDY2NyA0LjE0IDYuMDYgMy4zOTMzMyA2LjA2IDIuMzhDNi4wNiAxLjcxMzMzIDYuMzEzMzMgMS4xNzMzMyA2LjgyIDAuNzU5OTk5QzcuMzQgMC4zMzMzMzMgOC4wNDY2NyAwLjEyIDguOTQgMC4xMkM5LjY0NjY3IDAuMTIgMTAuMjkzMyAwLjI0NjY2NyAxMC44OCAwLjVMMTAuNSAxLjg4TDEwLjM4IDEuODJDMTAuMTQgMS43MjY2NyA5Ljk0NjY3IDEuNjYgOS44IDEuNjJDOS41MiAxLjU0IDkuMjMzMzMgMS41IDguOTQgMS41QzguNTggMS41IDguMyAxLjU3MzMzIDguMSAxLjcyQzcuOTEzMzMgMS44NTMzMyA3LjgyIDIuMDMzMzMgNy44MiAyLjI2QzcuODIgMi40ODY2NyA3LjkyNjY3IDIuNjczMzMgOC4xNCAyLjgyQzguMyAyLjk0IDguNjQ2NjcgMy4xMDY2NyA5LjE4IDMuMzJDOS44NDY2NyAzLjU3MzMzIDEwLjMzMzMgMy44OCAxMC42NCA0LjI0QzEwLjk2IDQuNiAxMS4xMiA1LjA0IDExLjEyIDUuNTZDMTEuMTIgNi4yMjY2NyAxMC44NjY3IDYuNzY2NjcgMTAuMzYgNy4xOEM5LjgxMzMzIDcuNjQ2NjcgOS4wNDY2NyA3Ljg4IDguMDYgNy44OEM3LjY3MzMzIDcuODggNy4yNjY2NyA3LjgyNjY3IDYuODQgNy43MkM2LjUwNjY3IDcuNjUzMzMgNi4yMDY2NyA3LjU2IDUuOTQgNy40NEw2LjMyIDZaIiBmaWxsPSIjQ0JDQjQxIi8+Cjwvc3ZnPgo=',
            },
            status: {
                icon: MynahIcons.PROGRESS,
                text: 'Working',
                status: 'warning',
            },
            buttons: [
                {
                    id: 'stop',
                    status: 'clear',
                    icon: MynahIcons.STOP,
                },
            ],
            fileList: {
                fileTreeTitle: '',
                filePaths: ['package.json'],
                details: {
                    'package.json': {
                        icon: null,
                        label: 'Creating',
                        changes: {
                            added: 36,
                            deleted: 0,
                            total: 36,
                        },
                    },
                },
            },
        },
    },
    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        buttons: [
            {
                id: 'undo-all',
                status: 'clear',
                position: 'outside',
                keepCardAfterClick: false,
                icon: MynahIcons.UNDO,
                text: 'Undo all changes',
            },
        ],
    },
    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        padding: false,
        header: {
            icon: 'code-block',
            status: {
                icon: MynahIcons.OK,
                text: 'Accepted',
                status: 'success',
            },
            fileList: {
                hideFileCount: true,
                fileTreeTitle: '',
                filePaths: ['package.json'],
                details: {
                    'package.json': {
                        icon: null,
                        label: 'Created',
                        changes: {
                            added: 36,
                            deleted: 0,
                            total: 36,
                        },
                    },
                },
            },
        },
    },

    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        padding: false,
        muted: true,
        header: {
            icon: 'code-block',
            status: {
                icon: MynahIcons.OK,
                text: 'Accepted',
                status: 'success',
            },
            fileList: {
                hideFileCount: true,
                fileTreeTitle: '',
                filePaths: ['package.json'],
                details: {
                    'package.json': {
                        icon: null,
                        label: 'Created',
                        changes: {
                            added: 36,
                            deleted: 0,
                            total: 36,
                        },
                    },
                },
            },
        },
    },

    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        padding: false,
        muted: true,
        header: {
            icon: 'code-block',
            status: {
                icon: MynahIcons.CANCEL,
                text: 'Rejected',
            },
            fileList: {
                hideFileCount: true,
                fileTreeTitle: '',
                filePaths: ['package.json'],
                details: {
                    'package.json': {
                        icon: null,
                        label: 'Created',
                        changes: {
                            added: 36,
                            deleted: 0,
                            total: 36,
                        },
                    },
                },
            },
        },
    },

    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        padding: false,
        muted: true,
        header: {
            icon: 'code-block',
            status: {
                icon: MynahIcons.ERROR,
                text: 'Error',
                status: 'error',
                description: 'There was an error while creating the file.',
            },
            fileList: {
                hideFileCount: true,
                fileTreeTitle: '',
                filePaths: ['package.json'],
                details: {
                    'package.json': {
                        icon: null,
                        label: 'Created',
                        changes: {
                            added: 36,
                            deleted: 0,
                            total: 36,
                        },
                    },
                },
            },
        },
    },

    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        padding: false,
        messageId: generateUID(),
        header: {
            icon: 'code-block',
            buttons: [
                {
                    icon: 'cancel',
                    status: 'clear',
                    id: 'reject-file-change-on-header-card',
                },
                {
                    icon: 'ok',
                    status: 'clear',
                    id: 'accept-file-change-on-header-card',
                },
            ],
            fileList: {
                hideFileCount: true,
                fileTreeTitle: '',
                filePaths: ['package.json'],
                details: {
                    'package.json': {
                        icon: null,
                        label: 'Created',
                        changes: {
                            added: 36,
                            deleted: 0,
                            total: 36,
                        },
                    },
                },
            },
        },
        body: `
\`\`\`bash
hello
\`\`\`
\`\`\`diff-typescript
const mynahUI = new MynahUI({
tabs: {
    'tab-1': {
        isSelected: true,
        store: {
            tabTitle: 'Chat',
            chatItems: [
                {
                    type: ChatItemType.ANSWER,
                    body: 'Welcome to our chat!',
                    messageId: 'welcome-message'
                },
            ],
-                promptInputPlaceholder: 'Write your question',
+                promptInputPlaceholder: 'Type your question',
        }
    }
},
-    onChatPrompt: () => {},
+    onChatPrompt: (tabId: string, prompt: ChatPrompt) => {
+        mynahUI.addChatItem(tabId, {
+            type: ChatItemType.PROMPT,
+            messageId: new Date().getTime().toString(),
+            body: prompt.escapedPrompt
+        });
+        // call your genAI action
+    }
});
\`\`\`
    `,
        codeBlockActions: {
            copy: null,
            'insert-to-cursor': null,
        },
    },

    {
        fullWidth: true,
        padding: false,
        type: ChatItemType.ANSWER,
        header: {
            icon: MynahIcons.SHELL,
            body: 'Terminal command',
            status: {
                icon: MynahIcons.PROGRESS,
            },
            buttons: [
                {
                    status: 'clear',
                    icon: MynahIcons.STOP,
                    id: 'stop-bash-command',
                },
            ],
        },
        body: `
\`\`\`bash
mkdir -p src/ lalalaaaa
\`\`\`
`,
        codeBlockActions: { copy: null, 'insert-to-cursor': null },
    },
    {
        fullWidth: true,
        padding: false,
        type: ChatItemType.ANSWER,
        header: {
            body: 'Shell',
            status: {
                position: 'left',
                icon: MynahIcons.WARNING,
                status: 'warning',
                description: 'This command may cause\nsignificant data loss or damage.',
            },
            buttons: [
                {
                    status: 'clear',
                    icon: 'play',
                    text: 'Run',
                    id: 'run-bash-command',
                },
                {
                    status: 'dimmed-clear',
                    icon: 'cancel',
                    text: 'Reject',
                    id: 'reject-bash-command',
                },
            ],
        },
        body: `
\`\`\`bash
mkdir -p src/ lalalaaaa
\`\`\`
`,
        quickSettings: {
            type: "select",
            messageId: "1",
            tabId: "hello",
            description: '',
            descriptionLink: {
                id: "button-id",
                destination: "Built-in",
                text: 'More control, modify the commands',
            },
            options: [
                { id: 'option1', label: 'Ask to Run', selected: true, value: 'Destructive' },
                { id: 'option2', label: 'Auto run', value: 'Destructive' },
            ],
            onChange: (selectedOptions: any) => {
                console.log('Selected options:', selectedOptions);
            }
    },
        codeBlockActions: { copy: null, 'insert-to-cursor': null },
    },
    {
        fullWidth: true,
        padding: false,
        type: ChatItemType.ANSWER,
        header: {
            icon: MynahIcons.CODE_BLOCK,
            body: 'Terminal command',
            buttons: [
                {
                    status: 'clear',
                    icon: 'play',
                    id: 'run-bash-command',
                },
            ],
        },
        body: `
\`\`\`bash
mkdir -p src/ lalalaaaa
\`\`\`
`,
        codeBlockActions: { copy: null, 'insert-to-cursor': null },
    },

    {
        fullWidth: true,
        padding: false,
        type: ChatItemType.ANSWER,
        header: {
            body: 'Shell',
            status: {
                position: 'left',
                icon: MynahIcons.WARNING,
                status: 'warning',
                description: 'This command may cause\nsignificant data loss or damage.',
            },
            buttons: [
                {
                    status: 'clear',
                    icon: 'play',
                    text: 'Run',
                    id: 'run-bash-command',
                },
                {
                    status: 'dimmed-clear',
                    icon: 'cancel',
                    text: 'Reject',
                    id: 'reject-bash-command',
                },
            ],
        },
        body: `
\`\`\`bash
mkdir -p src/ lalalaaaa
\`\`\`
`,
        codeBlockActions: { copy: null, 'insert-to-cursor': null },
    },

    {
        type: ChatItemType.DIRECTIVE,
        body: `Starting with a directive with normal text.`,
    },

    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        padding: false,
        header: {
            buttons: [
                {
                    icon: 'undo',
                    text: 'Undo',
                    status: 'clear',
                    id: 'undo-change', // Or whatever ID you have
                },
            ],
            fileList: {
                hideFileCount: true,
                fileTreeTitle: '',
                filePaths: ['maze_game.py'],
                details: {
                    'maze_game.py': {
                        description: 'Hello!',
                        icon: null,
                        changes: {
                            added: 131,
                            deleted: 0,
                        },
                    },
                },
            },
        },
    },
    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        padding: false,
        status: 'error',
        body: 'To avoid errors, do not make weird things in the system!',
        header: {
            icon: 'cancel',
            iconForegroundStatus: 'error',
            body: '##### Error on something!',
        },
    },
    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        padding: false,
        // status: '',
        body: 'To avoid warnings, do not make weird things in the system!',
        header: {
            status: {
                icon: 'warning',
                status: 'warning',
                position: 'left',
                description: 'There is an error!',
            },
            body: '##### Warning on something!',
            buttons: [
                {
                    id: 'accept-warning',
                    text: 'Accept',
                    status: 'clear',
                    icon: 'ok',
                },
            ],
        },
    },
    {
        fullWidth: true,
        padding: false,
        type: ChatItemType.ANSWER,
        wrapCodes: true,
        header: {
            icon: MynahIcons.CODE_BLOCK,
            body: 'Terminal command',
            buttons: [
                {
                    status: 'clear',
                    icon: 'play',
                    text: 'Run',
                    id: 'run-bash-command',
                },
                {
                    status: 'dimmed-clear',
                    icon: 'cancel',
                    text: 'Reject',
                    id: 'cancel-bash-command',
                },
            ],
        },
        body: `
\`\`\`bash
mkdir -p src/ lalalaaaa sad fbnsafsdaf sdakjfsd sadf asdkljf basdkjfh ksajhf kjsadhf dskjkj hasdklf askdjfh kj sadhfksdaf
\`\`\`
`,
        codeBlockActions: { copy: null, 'insert-to-cursor': null },
    },
    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        padding: false,
        header: {
            icon: 'progress',
            body: 'Reading',
            fileList: {
                filePaths: ['package.json', 'README.md'],
                details: {
                    'package.json': {
                        visibleName: 'package.json',
                        description: 'package.json'
                    },
                    'README.md': {
                        visibleName: 'README.md',
                        description: 'README.md'
                    }
                },
                renderAsPills: true
            }
        },
    },
    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        padding: false,
        header: {
            icon: 'eye',
            body: '5 files read',
            fileList: {
                filePaths: ['package.json', 'README.md', 'webpack.config.js', 'src/app.ts', 'src/components/Button/Button.tsx'],
                details: {
                    'package.json': {
                        visibleName: 'package.json',
                        description: 'package.json'
                    },
                    'README.md': {
                        visibleName: 'README.md',
                        description: 'README.md'
                    },
                    'webpack.config.js': {
                        visibleName: 'webpack.config.js',
                        description: 'webpack.config.js'
                    },
                    'src/app.ts': {
                        visibleName: 'app.ts',
                        description: 'src/app.ts'
                    },
                    'src/components/Button/Button.tsx': {
                        visibleName: 'Button.tsx',
                        description: 'src/components/Button/Button.tsx'
                    }
                },
                renderAsPills: true
            }
        },
    },
    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        padding: false,
        header: {
            icon: 'progress',
            body: 'Listing',
            fileList: {
                filePaths: ['src/components/ui', 'src/components/forms'],
                details: {
                    'src/components/ui': {
                        visibleName: 'ui',
                        description: 'src/components/ui',
                        clickable: false
                    },
                    'src/components/forms': {
                        visibleName: 'forms',
                        description: 'src/components/forms',
                        clickable: false
                    },
                },
                renderAsPills: true
            }
        }
    },
    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        padding: false,
        header: {
            icon: 'check-list',
            body: '5 directories listed',
            fileList: {
                filePaths: ['src/components/ui', 'src/components/forms', 'src/components/layout', 'src/utils/helpers', 'src/utils/validation'],
                details: {
                    'src/components/ui': {
                        visibleName: 'ui',
                        description: 'src/components/ui',
                        clickable: false
                    },
                    'src/components/forms': {
                        visibleName: 'forms',
                        description: 'src/components/forms',
                        clickable: false
                    },
                    'src/components/layout': {
                        visibleName: 'layout',
                        description: 'src/components/layout',
                        clickable: false
                    },
                    'src/utils/helpers': {
                        visibleName: 'helpers',
                        description: 'src/components/helpers',
                        clickable: false
                    },
                    'src/utils/validation': {
                        visibleName: 'validation',
                        description: 'src/components/validation',
                        clickable: false
                    },
                },
                renderAsPills: true
            }
        }
    },
    {
        type: ChatItemType.ANSWER,
        fullWidth: true,
        padding: false,
        header: {
            icon: 'search',
            body: 'Searched for `*.md` in',
            fileList: {
                filePaths: ['src/docs'],
                details: {
                    ['src/docs']: {
                        visibleName: 'docs',
                        description: 'src/docs',
                        clickable: false
                    }
                },
                renderAsPills: true
            },
            status: {
                text: '5 results found'
            }
        },
    }
];


export const mcpToolRunSampleCardInit:ChatItem = // Summary Card
{
    padding: false,
    type: ChatItemType.ANSWER,
    summary: {
        content: {
            padding: false,
            wrapCodes: true,
            header: {
                icon: MynahIcons.TOOLS,
                fileList: {
                    hideFileCount: true,
                    fileTreeTitle: '',
                    filePaths: ['Running'],
                    details: {
                        'Running': {
                            description: 'Work in progress!',
                            icon: null,
                            labelIcon: 'progress',
                            labelIconForegroundStatus: 'info',
                            label: 'Filesystem tool search-files'
                        },
                    },
                },
            },
        },
        collapsedContent: [
            {
                fullWidth: true,
                padding: false,
                muted: true,
                wrapCodes: true,
                header: {
                    body: 'Parameters',
                },
                body: `
\`\`\`json
{
"query": "user:zakiyav",
"perPage": 100,
"page": 1
}
\`\`\`
    `,
                codeBlockActions: { copy: null, 'insert-to-cursor': null },
            },
        ],
    },
};
export const mcpToolRunSampleCard:ChatItem = // Summary Card
{
    padding: false,
    type: ChatItemType.ANSWER,
    summary: {
        content: {
            padding: false,
            wrapCodes: true,
            header: {
                icon: MynahIcons.TOOLS,
                body: 'Ran Filesystem tool search-files',
                fileList: null,
                buttons: [
                {
                    status: 'clear',
                    icon: 'play',
                    text: 'Run',
                    id: 'run-bash-command',
                },
                {
                    status: 'dimmed-clear',
                    icon: 'cancel',
                    text: 'Reject',
                    id: 'reject-bash-command',
                },
            ],
            },
            quickSettings: {
                type: "select",
                messageId: "1",
                tabId: "hello",
                description: '',
                descriptionLink: {
                    id: "button-id",
                    destination: "Built-in",
                    text: 'Auto-approve settings',
                },
                options: [
                    { id: 'option1', label: 'Ask to Run', selected: true, value: 'Destructive' },
                    { id: 'option2', label: 'Auto run', value: 'Destructive' },
                ],
                onChange: (selectedOptions: any) => {
                    console.log('Selected options:', selectedOptions);
                }
            },
        },
        collapsedContent: [
            {
                fullWidth: true,
                padding: false,
                muted: true,
                wrapCodes: true,
                header: {
                    body: 'Parameters',
                },
                body: `
\`\`\`json
{
"query": "user:zakiyav",
"perPage": 100,
"page": 1
}
\`\`\`
    `,
                codeBlockActions: { copy: null, 'insert-to-cursor': null },
            },
            {
                fullWidth: true,
                padding: false,
                muted: true,
                wrapCodes: false,
                header: {
                    body: 'Results',
                },
                body: `
\`\`\`json
{ 
  "total_count": 1, 
  "incomplete_results": false, 
  "items": [
    { 
        "id": 69836433, 
        "node_id": "MDasflJlcG9zaXRvcasdnk2OTgzN", 
        "name": "Repo1", 
        "full_name": "zakiyav/Repo1", 
        "private": true, 
        "owner": { 
            "login": "zakiyav", 
            "id": 5873805510, 
            "node_id": "MasDQgb6VjXNlcjQ5MzU1sfasMTA=", 
            "avatar_url": "https://avatars.githubus?v=4", 
            "url": "https://api.github.com/users/zakiyav", 
            "html_url": "https://github.com/zakiyav", 
            "type": "User" 
        } 
        "default_branch": "master" 
    } 
  ]
}
\`\`\`
    `,
                codeBlockActions: { copy: null, 'insert-to-cursor': null },
            },
        ],
    },
};

export const sampleMCPList: DetailedList = {
    selectable: 'clickable',
    header: {
        title: 'MCP Servers',
        status: {},
        description:
            'Q automatically uses any MCP servers that have been added, so you don\'t have to add them as context. All MCPs are defaulted to "Ask before running".',
        actions: [
            {
                id: 'add-new-mcp',
                icon: 'plus',
                status: 'clear',
                description: 'Add new MCP',
            },
            {
                id: 'refresh-mcp-list',
                icon: 'refresh',
                status: 'clear',
                description: 'Refresh MCP servers',
            },
        ],
    },
    textDirection: 'row',
    list: [
        {
            groupName: 'Active',
            children: [
                {
                    title: 'Built-in',
                    icon: 'ok-circled',
                    status: {
                        description: '8 available tools',
                        icon: 'tools',
                        text: '8',
                    },
                    iconForegroundStatus: 'success',
                    actions: [
                        {
                            id: 'open-mcp-xx',
                            icon: 'right-open',
                        },
                    ],
                },
                {
                    title: 'Filesystem',
                    icon: 'ok-circled',
                    status: {
                        icon: 'tools',
                        description: '26 available tools',
                        text: '26',
                    },
                    iconForegroundStatus: 'success',
                    actions: [
                        {
                            id: 'open-mcp-xx',
                            icon: 'right-open',
                        },
                    ],
                },
                {
                    title: 'Git',
                    icon: 'cancel-circle',
                    iconForegroundStatus: 'error',
                    description: 'Configuration is broken',
                    groupActions: false,
                    actions: [
                        {
                            id: 'open-mcp-xx',
                            text: 'Fix configuration',
                            icon: 'pencil',
                        },
                        {
                            id: 'open-mcp-xx',
                            disabled: true,
                            icon: 'right-open',
                        },
                    ],
                },
                {
                    title: 'Github',
                    icon: 'progress',
                    iconForegroundStatus: 'info',
                    groupActions: false,
                    actions: [
                        {
                            id: 'open-mcp-xx',
                            icon: 'right-open',
                        },
                    ],
                },
            ],
        },
        {
            groupName: 'Disabled',
            children: [
                {
                    title: 'Redis',
                    icon: 'block',
                    groupActions: false,
                    actions: [
                        {
                            id: 'mcp-enable-tool',
                            icon: MynahIcons.OK,
                            text: 'Enable',
                        },
                        {
                            id: 'mcp-delete-tool',
                            icon: MynahIcons.TRASH,
                            text: 'Delete',
                            confirmation: {
                                cancelButtonText: 'Cancel',
                                confirmButtonText: 'Delete',
                                title: 'Delete Filesystem MCP server',
                                description:
                                    'This configuration will be deleted and no longer available in Q. \n\n **This cannot be undone.**',
                            },
                        },
                        {
                            id: 'open-mcp-xx',
                            disabled: true,
                            icon: 'right-open',
                        },
                    ],
                },
            ],
        },
    ],
    filterOptions: [],
    filterActions: [],
};

export const sampleMCPDetails = (title: string): DetailedList => {
    return {
        header: {
            title: `MCP: ${title}`,
            status: {
                title: 'Detail of the issue',
                icon: 'cancel-circle',
                status: 'error',
            },
            description:
                'Extend the capabilities of Q with [MCP servers](#). Q automatically uses any MCP server that has been added. All MCPs are defaulted to "Ask before running". [Learn more](#)',
            actions: [
                {
                    icon: 'pencil',
                    text: 'Edit setup',
                    id: 'mcp-edit-setup',
                },
                {
                    icon: 'ellipsis-h',
                    id: 'mcp-details-menu',
                    items: [
                        {
                            id: 'mcp-disable-tool',
                            text: `Disable ${title}`,
                            icon: 'block',
                        },
                        {
                            id: 'mcp-delete-tool',
                            confirmation: {
                                cancelButtonText: 'Cancel',
                                confirmButtonText: 'Delete',
                                title: 'Delete Filesystem MCP server',
                                description:
                                    'This configuration will be deleted and no longer available in Q. \n\n This cannot be undone.',
                            },
                            text: `Delete ${title}`,
                            icon: 'trash',
                        },
                    ],
                },
            ],
        },
        list: [],
        filterActions: [
            {
                id: 'cancel-mcp',
                text: 'Cancel',
            },
            {
                id: 'save-mcp',
                text: 'Save',
                status: 'primary',
            },
        ],
        filterOptions: [
            {
                type: 'select',
                id: 'auto-approve',
                title: 'Auto Approve',
                options: [
                    {
                        label: 'Yes',
                        value: 'yes',
                    },
                    {
                        label: 'No',
                        value: 'no',
                    },
                ],
                selectTooltip: "Permission for this tool is not configurable yet",
                mandatory: true,
                disabled: true,
                hideMandatoryIcon: true,
                boldTitle: true,
            },
          {
                type: 'select',
                id: 'tool_name',
                title: 'Tool Name',
                value: 'alwaysAllow',
                options: [
                    {
                        label: 'Ask',
                        value: 'ask',
                        description: "Ask for your approval each time this tool is run"
                    },
                    {
                        label: 'Always Allow',
                        value: 'alwaysAllow',
                        description: 'Always allow this tool to run without asking for approval'
                    },
                    {
                        label: 'Deny',
                        value: 'deny',
                        description: 'Never run this tool'
                    },
                ],
                boldTitle: true,
                mandatory: true,
                hideMandatoryIcon: true,
            },
            {
                type: 'select',
                id: 'transport',
                title: 'Transport',
                options: [
                    {
                        label: 'Yes',
                        value: 'yes',
                    },
                    {
                        label: 'No',
                        value: 'no',
                    },
                ]
            },
            {
                type: 'textinput',
                title: 'Command',
                id: 'command',
            },
            {
                type: 'numericinput',
                title: 'Timeout',
                description: 'Seconds',
                id: 'timeout',
            },
            { // Add mandatory field
                id: 'args-pillbox',
                type: 'pillbox',
                title: 'Arguments - pillbox',
                placeholder: 'Type arguments and press Enter',
                value: '-y,@modelcontextprotocol/server-filesystem,/Users/username/Desktop,/path/to/other/allowed/dir',
            },
            {
                id: 'args',
                type: 'list',
                title: 'Arguments',
                mandatory: false,
                items: [
                    {
                        id: 'arg_key',
                        type: 'textinput',
                    },
                ],
                value: [
                    {
                        persistent: true,
                        value: {
                            arg_key: '-y',
                        },
                    },
                    {
                        value: {
                            arg_key: '@modelcontextprotocol/server-filesystem',
                        },
                    },
                    {
                        value: {
                            arg_key: '/Users/username/Desktop',
                        },
                    },
                    {
                        value: {
                            arg_key: '/path/to/other/allowed/dir',
                        },
                    },
                ],
            },
            {
                id: 'env_variables',
                type: 'list',
                title: 'Environment variables',
                items: [
                    {
                        id: 'env_var_name',
                        title: 'Name',
                        type: 'textinput',
                    },
                    {
                        id: 'env_var_value',
                        title: 'Value',
                        type: 'textinput',
                    },
                ],
                value: [
                    {
                        value: {
                            env_var_name: 'some_env',
                            env_var_value: 'AJSKJLE!@)(UD',
                        },
                    },
                    {
                        value: {
                            env_var_name: 'some_other_env',
                            env_var_value: '12kjlkj!dddaa',
                        },
                    },
                ],
            },
        ],
    };
};


export const sampleRulesList: DetailedList = {selectable: 'clickable', list: [{children: [{id: 'README', icon: MynahIcons.CHECK_LIST, 
    description: 'README',actions: [{ id: 'README.md', icon: MynahIcons.OK, status: 'clear' }]}]}, 
    {groupName: '.amazonq/rules', childrenIndented: true, icon: MynahIcons.FOLDER , actions: [{ id: 'java-expert.md', icon: MynahIcons.OK, status: 'clear' }], children: [{id: 'java-expert.md', icon: MynahIcons.CHECK_LIST, 
    description: 'java-expert',actions: [{ id: 'java-expert.md', icon: MynahIcons.OK, status: 'clear' }]}]}]}