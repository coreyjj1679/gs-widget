"""Player nationality required to create a monster at the training center.

Keys identify the result of the promotion. Zero means all nationalities are
eligible. An absent key means the requirements have not been recorded.
"""

MONSTER_PROMOTION_NATIONALITY = {
    # The publisher specifies that each of these 12 combinations must be made
    # at the training center belonging to the player's own country.
    # https://www.inside-games.jp/article/2008/03/17/27836.html
    "E33": 1,  # Korea
    "E34": 1,
    "E35": 1,
    "E36": 10,  # Japan
    "E37": 10,
    "E38": 10,
    "E42": 100,  # China
    "E43": 100,
    "E44": 100,
    "E39": 1000,  # Taiwan
    "E40": 1000,
    "E41": 1000,
    # Both stages preceding Airavata are open to any nationality. The nine
    # initial monsters still have designated Indian training centers.
    # https://noninol.tistory.com/600
    "D78": 0,  # Mongoose
    "D79": 0,  # Greatsword Bandit
    "F1": 0,   # Twinblade Thief
    "F2": 0,   # Beetle
    "F3": 0,   # Land Tortoise
    "F4": 0,   # Bengal Fox
    "F9": 0,   # Two-headed Snake
    "F14": 0,  # Ox Porter (promoted from Donkey)
    "F15": 0,  # Jungle Cat
    "F31": 0,  # Tiamat
    "F32": 0,  # Scout
    "F33": 0,  # Bullman
    # Four regional fiends require the corresponding player's nationality.
    # https://www.gersang.co.kr/community/knowledge_view.gs?GSbid=4003&GSuid=25627&page=1
    "D58": 1,     # Hundun
    "D57": 10,    # Qiongqi
    "D55": 100,   # Taowu
    "D56": 1000,  # Taotie
    # Catalog patch V28600 introduced the Indian nationality requirement.
    # The publisher then exempted players who completed their second promotion.
    # https://gs.mangot5.com/game/gs/notice/detail?contentNo=44111
    "T79": "india_or_second_promotion",  # Airavata
}
