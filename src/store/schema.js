import vanillaData from './vanilla-content.json'


export const TYPES = ['item', 'liquid', 'block', 'unit', 'status', 'weather', 'team']

export const TYPE_LABEL = {
  zh: { item: '物品', liquid: '液体', block: '方块', unit: '单位', status: '状态', weather: '天气', team: '队伍', planet: '星球', sector: '区块' },
  en: { item: 'Item', liquid: 'Liquid', block: 'Block', unit: 'Unit', status: 'Status', weather: 'Weather', team: 'Team', planet: 'Planet', sector: 'Sector' },
}

export const TYPE_COLOR = {
  item: '#D99D73',
  liquid: '#4747FF',
  block: '#8C7FA9',
  unit: '#9CB664',
  status: '#FF795E',
  weather: '#6ECDEC',
  team: '#F3E979',
  planet: '#7FB77E',
  sector: '#E9C46A',
}

export const vanillaNames = vanillaData.content

export function isVanilla(type, name) {
  return (vanillaNames[type] || []).includes(name)
}
