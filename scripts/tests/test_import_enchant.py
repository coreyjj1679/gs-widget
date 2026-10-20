import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from import_enchant import parse_enchant, parse_npcs


RECIPE = """
; backup
#아이템 코드	아이템 이름	Grade	Riddle	Reinforcement	Resonance	이름 코드	그룹	단위	단위 코드	설명문	설명문 코드	가격	무게	최대	그림	폰트	필드	필드폰트	Cost	사운드	신용	유료	연마 성공 확률(실수형)	사용 조건	재료 아이템 테이블 인덱스	연마되는 아이템 종류
1129	[고구려의] 주술 비법	0	0	0	0	2398	E	개	48	설명	2618	1	1	100	a.agf	0	b.agf	0	10	ETC	1	0	15	Level150	4Cls_4	TAEHWANG_ARMOR, TAEHWANG_HELMET
;1128	옛 행	0	0	0	0	0	E	개	48	설명	0	1	1	100	a.agf	0	b.agf	0	10	ETC	1	0	99	Level1	Basic	ALL
"""

MATERIALS = """
#테이블 인덱스	재료1	재료1 코드	재료1 수량	재료2	재료2 코드	재료2 수량	재료3	재료3 코드	재료3 수량	재료4	재료4 코드	재료4 수량	재료5	재료5 코드	재료5 수량	재료6	재료6 코드	재료6 수량	재료7	재료7 코드	재료7 수량	재료8	재료8 코드	재료8 수량	재료9	재료9 코드	재료9 수량	재료10	재료10 코드	재료10 수량
4Cls_4	적색 정기의 돌	2206	20	흑철괴	2287	10	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0	0
"""

TARGETS = """
#아이템 종류 어휘	설명(사용안함)	설명 텍스트 인덱스
TAEHWANG_ARMOR	태황갑옷	6889	;주석
TAEHWANG_HELMET	태황투구	6892
"""

ITEMS = {
    1129: "[고구려의] 주술 비법",
    2206: "적색 정기의 돌",
    2287: "흑철괴",
}
STRINGS = {6889, 6892}


class EnchantParseTests(unittest.TestCase):
    def parse(self, recipe=RECIPE, materials=MATERIALS, targets=TARGETS):
        return parse_enchant(recipe, materials, targets, ITEMS, STRINGS)

    def test_skips_comments_and_keeps_codes_quantities_and_targets(self):
        recipes, materials, types, links = self.parse()
        self.assertEqual(recipes, [(1129, 15, 150, "4Cls_4")])
        self.assertEqual(materials, [("4Cls_4", 1, 2206, 20), ("4Cls_4", 2, 2287, 10)])
        self.assertEqual(types, [("TAEHWANG_ARMOR", 6889, 0), ("TAEHWANG_HELMET", 6892, 0)])
        self.assertEqual(links, [(1129, 1, "TAEHWANG_ARMOR"), (1129, 2, "TAEHWANG_HELMET")])

    def test_rejects_a_material_whose_name_does_not_match_its_code(self):
        broken = MATERIALS.replace("흑철괴", "다른물건")
        with self.assertRaises(ValueError):
            self.parse(materials=broken)

    def test_rejects_an_unknown_target_token(self):
        with self.assertRaises(ValueError):
            self.parse(recipe=RECIPE.replace("TAEHWANG_HELMET", "MISSING_HELMET"))


NPC = """
#NPC 코드	NPC 이름	NPC 등급	점괘 비용	주술연마 비용(읽지않음)	주술 연마 성공확률(실수형)	tarot param code	NPC 초상화 인덱스	NPC 인사말1	NPC 인사말2	NPC 인사말3	NPC 점괘 대사1	NPC 점괘 대사2	NPC 점괘 대사3	NPC 주술연마 대사1	NPC 주술연마 대사2	NPC 주술연마 대사3	주술연마 비용(1등급)	주술연마 비용(2등급)	주술연마 비용(3등급)	주술연마 비용(4등급)	주술연마 비용(5등급)	주술연마 비용(6등급)	주술연마 비용(7등급)
37	아무개 산신령	1	10000	5200	2500000	1Cls_EnchantChosun	58	2913	2914	2915	2916	2917	2918	2919	2920	2921	10000	50000	100000	300000	1000000	1000000	1000000
"""

BONUS = """
#Parm Code	TarotPrb. Share Rate	Inc.TarotPrb. Per Req.Level	Inc.TarotPrb. Per Req.Credit	fTarotPerIsZ	fTarotPerIsO	fTarotPerIsI	fTarotPerIsY	fTarotPerIsL	fTarotPerIsT	fTarotPerIsC	fTarotPerIsU	fTarotPerIsBelt	fTarotPerIsRing	fTarotPerIsShoes	fTarotPerIsArmor	fTarotPerIsGlove	fTarotPerIsHelmet	fTarotPerIsElement	fTarotPerIsFood	fTarotPerIsWeapon
1Cls_EnchantChosun	98	10000	10000	20000	20000	20000	10000	10000	40000	20000	25000	10000	10000	10000	10000	10000	10000	40000	35000	10000
"""


class EnchantNpcTests(unittest.TestCase):
    def test_keeps_fees_and_fortune_weights_without_the_unread_fee(self):
        npcs, bonuses = parse_npcs(NPC, BONUS, {37: "[전체:주술연마] 조선 아무개 산신령"})
        self.assertEqual(npcs, [(37, 1, 10000, 2500000, "1Cls_EnchantChosun", 10000, 50000, 100000, 300000, 1000000, 1000000, 1000000)])
        self.assertEqual(bonuses[0][0], "1Cls_EnchantChosun")
        self.assertEqual(bonuses[0][1], 98)
        self.assertNotIn(5200, npcs[0])


if __name__ == "__main__":
    unittest.main()
