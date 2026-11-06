import { describe, expect, it } from 'vitest'
import {
  CLICK_AREA_ACTIONS,
  CLICK_AREA_PROFILES,
  CLICK_AREA_PROFILE_LABELS,
  CLICK_AREA_ZONES,
  clickAreaActionLabel,
  clickAreaZoneAt,
  defaultClickAreaModes,
  defaultClickAreaProfiles,
  hasMenuZone,
  migrateLegacyClickAreas,
  normalizeClickAreaModes,
  normalizeClickAreaProfiles,
  normalizeClickAreas,
  pagedClickAreas,
  presetClickAreas,
  scrollClickAreas,
  setClickAreaAction,
  type ClickAreaMap,
} from './readerClickAreas'

describe('clickAreaZoneAt', () => {
  it('按 3x3 切分坐标比例', () => {
    expect(clickAreaZoneAt(0.1, 0.1)).toBe('tl')
    expect(clickAreaZoneAt(0.5, 0.1)).toBe('tc')
    expect(clickAreaZoneAt(0.9, 0.1)).toBe('tr')
    expect(clickAreaZoneAt(0.1, 0.5)).toBe('ml')
    expect(clickAreaZoneAt(0.5, 0.5)).toBe('mc')
    expect(clickAreaZoneAt(0.9, 0.5)).toBe('mr')
    expect(clickAreaZoneAt(0.1, 0.9)).toBe('bl')
    expect(clickAreaZoneAt(0.5, 0.9)).toBe('bc')
    expect(clickAreaZoneAt(0.9, 0.9)).toBe('br')
  })

  it('边界值归入后一格（与 RectF.contains 的左闭右开一致）', () => {
    // 1/3 恰好落在分界线上：手机端的矩形是 [left, right)，所以归第二格。
    expect(clickAreaZoneAt(1 / 3, 1 / 3)).toBe('mc')
    expect(clickAreaZoneAt(2 / 3, 2 / 3)).toBe('br')
    // 坐标可能因浮点误差略微越界，仍要落回九宫格内。
    expect(clickAreaZoneAt(1, 1)).toBe('br')
    expect(clickAreaZoneAt(-0.2, 1.5)).toBe('bl')
  })
})

describe('预设', () => {
  it('左右翻页预设按列分区', () => {
    const areas = pagedClickAreas()
    expect(areas.tl).toBe('prevPage')
    expect(areas.mc).toBe('menu')
    expect(areas.br).toBe('nextPage')
    // 同一列三格动作一致，翻页时上下点都能翻。
    expect(new Set([areas.tl, areas.ml, areas.bl])).toEqual(new Set(['prevPage']))
    expect(new Set([areas.tr, areas.mr, areas.br])).toEqual(new Set(['nextPage']))
  })

  it('上下滚动预设按行分区', () => {
    const areas = scrollClickAreas()
    expect(new Set([areas.tl, areas.tc, areas.tr])).toEqual(new Set(['prevPage']))
    expect(new Set([areas.ml, areas.mc, areas.mr])).toEqual(new Set(['menu']))
    expect(new Set([areas.bl, areas.bc, areas.br])).toEqual(new Set(['nextPage']))
  })

  it('两种预设都能呼出菜单', () => {
    expect(hasMenuZone(pagedClickAreas())).toBe(true)
    expect(hasMenuZone(scrollClickAreas())).toBe(true)
    expect(presetClickAreas('paged')).toEqual(pagedClickAreas())
    expect(presetClickAreas('scroll')).toEqual(scrollClickAreas())
  })

  it('每次返回新对象，改一份不会污染另一份', () => {
    const a = scrollClickAreas()
    a.mc = 'none'
    expect(scrollClickAreas().mc).toBe('menu')
    expect(pagedClickAreas().mc).toBe('menu')
  })
})

describe('normalizeClickAreas', () => {
  const fallback = scrollClickAreas()

  it('缺格与脏值回落 fallback，其余保留', () => {
    const result = normalizeClickAreas({ tl: 'bookmark', mc: '不是动作' }, fallback)
    expect(result.tl).toBe('bookmark')
    expect(result.mc).toBe(fallback.mc)
    expect(result.br).toBe(fallback.br)
  })

  it('非对象输入整张回落', () => {
    for (const raw of [null, undefined, 'x', 42, [], [1, 2, 3]]) {
      expect(normalizeClickAreas(raw, fallback)).toEqual(fallback)
    }
  })

  it('不认识的键被忽略，不写进结果', () => {
    const result = normalizeClickAreas({ xx: 'menu', tl: 'menu' }, fallback)
    expect(Object.keys(result).sort()).toEqual([...CLICK_AREA_ZONES].sort())
  })

  it('允许把某一格设成 none（用户明确要留白）', () => {
    expect(normalizeClickAreas({ mc: 'none' }, fallback).mc).toBe('none')
  })
})

describe('setClickAreaAction', () => {
  it('正常改一格不影响其它格', () => {
    const before = scrollClickAreas()
    const { areas, forcedCenterMenu } = setClickAreaAction(before, 'tl', 'bookmark')
    expect(areas.tl).toBe('bookmark')
    expect(areas.mc).toBe('menu')
    expect(forcedCenterMenu).toBe(false)
    // 纯函数：不能就地改入参。
    expect(before.tl).toBe('prevPage')
  })

  it('九格都没菜单时把正中抢回菜单', () => {
    const noMenu: ClickAreaMap = {
      tl: 'prevPage', tc: 'prevPage', tr: 'nextPage',
      ml: 'prevPage', mc: 'prevPage', mr: 'nextPage',
      bl: 'bookmark', bc: 'nextPage', br: 'nextPage',
    }
    // 把最后一个菜单格改成无操作 —— 结果必须仍有菜单出口。
    const withMenu: ClickAreaMap = { ...noMenu, mc: 'menu' }
    const { areas, forcedCenterMenu } = setClickAreaAction(withMenu, 'mc', 'none')
    expect(forcedCenterMenu).toBe(true)
    expect(areas.mc).toBe('menu')
    expect(hasMenuZone(areas)).toBe(true)
  })

  it('只是把某格从菜单改成别的、但别处仍有菜单时不干预', () => {
    const areas: ClickAreaMap = { ...scrollClickAreas(), tl: 'menu' }
    const result = setClickAreaAction(areas, 'mc', 'none')
    expect(result.forcedCenterMenu).toBe(false)
    expect(result.areas.mc).toBe('none')
    expect(result.areas.tl).toBe('menu')
  })
})

describe('标签', () => {
  it('每个动作都有可读名', () => {
    for (const action of CLICK_AREA_ACTIONS) {
      expect(clickAreaActionLabel(action.id)).toBe(action.label)
    }
  })

  it('未知动作回落「无操作」而不是 undefined', () => {
    expect(clickAreaActionLabel(undefined)).toBe('无操作')
    expect(clickAreaActionLabel('nope' as never)).toBe('无操作')
  })

  it('动作 id 不重复', () => {
    const ids = CLICK_AREA_ACTIONS.map((action) => action.id)
    expect(new Set(ids).size).toBe(ids.length)
  })
})

describe('两套九宫格（左右翻页 / 上下滚动）', () => {
  it('两套预设各自独立，改一套不影响另一套', () => {
    const profiles = defaultClickAreaProfiles()
    expect(profiles.paged).toEqual(pagedClickAreas())
    expect(profiles.scroll).toEqual(scrollClickAreas())
    expect(profiles.paged).not.toBe(profiles.scroll)
    profiles.paged.tl = 'bookmark'
    expect(profiles.scroll.tl).toBe('prevPage')
    // 再取一次仍然是干净的默认值。
    expect(defaultClickAreaProfiles().paged.tl).toBe('prevPage')
  })

  it('两套默认都用默认三分区', () => {
    expect(defaultClickAreaModes()).toEqual({ paged: 'auto', scroll: 'auto' })
  })

  it('每种模式都有自己的名字', () => {
    expect(CLICK_AREA_PROFILES).toEqual(['paged', 'scroll'])
    expect(CLICK_AREA_PROFILE_LABELS.paged).toBe('左右翻页')
    expect(CLICK_AREA_PROFILE_LABELS.scroll).toBe('上下滚动')
  })
})

describe('normalizeClickAreaProfiles / normalizeClickAreaModes', () => {
  const fallback = defaultClickAreaProfiles()

  it('缺一套只回落那一套，另一套照常保留', () => {
    const result = normalizeClickAreaProfiles({
      paged: { tl: 'catalog' },
    }, fallback)
    expect(result.paged.tl).toBe('catalog')
    // paged 其余格回落预设。
    expect(result.paged.mc).toBe('menu')
    // scroll 整套缺失 → 回落预设。
    expect(result.scroll).toEqual(fallback.scroll)
  })

  it('脏值与空值整张回落', () => {
    for (const raw of [null, undefined, 'x', 42, []]) {
      expect(normalizeClickAreaProfiles(raw, fallback)).toEqual(fallback)
    }
  })

  it('开关只认 custom，其余按 auto', () => {
    expect(normalizeClickAreaModes({ paged: 'custom', scroll: 'nope' }))
      .toEqual({ paged: 'custom', scroll: 'auto' })
    expect(normalizeClickAreaModes(null)).toEqual({ paged: 'auto', scroll: 'auto' })
    expect(normalizeClickAreaModes({ paged: 'auto', scroll: 'custom' }))
      .toEqual({ paged: 'auto', scroll: 'custom' })
  })
})

describe('migrateLegacyClickAreas', () => {
  const fallback = defaultClickAreaProfiles()

  it('老存档的同一张网格铺到两套上，两套都是 custom', () => {
    const legacy = {
      tl: 'catalog' as const, tc: 'menu' as const, tr: 'search' as const,
      ml: 'prevPage' as const, mc: 'menu' as const, mr: 'nextPage' as const,
      bl: 'bookmark' as const, bc: 'menu' as const, br: 'tts' as const,
    }
    const { areas, modes } = migrateLegacyClickAreas(legacy, 'custom', fallback)
    expect(modes).toEqual({ paged: 'custom', scroll: 'custom' })
    expect(areas.paged).toEqual(legacy)
    expect(areas.scroll).toEqual(legacy)
    // 必须是两份独立对象，否则改一套会串到另一套。
    expect(areas.paged).not.toBe(areas.scroll)
  })

  it('老存档是 auto 时两套都保持 auto', () => {
    const { areas, modes } = migrateLegacyClickAreas({ tl: 'catalog' }, 'auto', fallback)
    expect(modes).toEqual({ paged: 'auto', scroll: 'auto' })
    expect(areas.paged.tl).toBe('catalog')
    expect(areas.scroll.tl).toBe('catalog')
  })

  it('老存档网格本身是脏的也能拿到可用的两套', () => {
    const { areas } = migrateLegacyClickAreas({ tl: '不存在' }, 'custom', fallback)
    expect(areas.paged.tl).toBe('prevPage')
    expect(areas.scroll.tl).toBe('prevPage')
  })
})
