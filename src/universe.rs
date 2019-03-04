/*  Copyright 2017-2018 the Conwayste Developers.
 *
 *  This file is part of libconway.
 *
 *  libconway is free software: you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation, either version 3 of the License, or
 *  (at your option) any later version.
 *
 *  libconway is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with libconway.  If not, see <http://www.gnu.org/licenses/>. */

use std::{char, cmp, fmt};

use crate::grids::{BitGrid, BitOperation, CharGrid};

use crate::rle::{Pattern, NO_OP_CHAR};

type UniverseError = String;

/// Builder paradigm to create `Universe` structs with default values.
pub struct BigBang {
    width:           usize,
    height:          usize,
    is_server:       bool,
    history:         usize,
    num_players:     usize,
    player_writable: Vec<Region>,
    fog_radius:      usize
}

/// Player builder
pub struct PlayerBuilder {
    writable_region: Region,
}

impl PlayerBuilder {
    /// Returns a new PlayerBuilder.
    pub fn new(region: Region) -> PlayerBuilder {
        PlayerBuilder {
            writable_region: region,
        }
    }
}

/// This is a builder for `Universe` structs.
/// 
/// # Examples
/// 
/// ```
/// let mut uni = conway::universe::BigBang::new()
///                 .width(512)      // optionally override width
///                 .height(256)     // optionally override height
///                 .fog_radius(16)  // optionally override fog radius
///                 .birth()
///                 .unwrap();
/// ```
impl BigBang {
    /// Creates and returns a new builder.
    pub fn new() -> BigBang {
        BigBang {
            width: 256,
            height: 128,
            is_server: true,
            history: 16,
            num_players: 0,
            player_writable: vec![],
            fog_radius: 6,
        }
    }

    /// Update the total number of columns for this Universe
    pub fn width(mut self, new_width: usize) -> BigBang {
        self.width = new_width;
        self
    }

    /// Update the total number of rows for this Universe
    pub fn height(mut self, new_height: usize) -> BigBang {
        self.height = new_height;
        self
    }

    /// Determines whether we are running a Server or a Client.
    /// * `true` - Server
    /// * `false` - Client
    pub fn server_mode(mut self, is_server: bool) -> BigBang {
        self.is_server = is_server;
        self
    }

    /// This records the number of generations that will be buffered.
    pub fn history(mut self, history_depth: usize) -> BigBang {
        self.history = history_depth;
        self
    }

    /// This will add a single player to the list of players. Each player is responsible for
    /// providing their details through the PlayerBuilder.
    /// 
    /// # Panics
    /// 
    /// Panics if, after adding this player, the length of the internal `player_writable` vector
    /// does not match the number of players.
    pub fn add_player(mut self, new_player: PlayerBuilder) -> BigBang {
        self.num_players += 1;
        self.player_writable.push(new_player.writable_region);
        assert_eq!(self.num_players, self.player_writable.len()); // These should always match up!
        self
    }

    /// Adds a vector of players using `add_player` method.
    /// 
    /// # Panics
    /// 
    /// Panics if, after adding these players, the length of the internal `player_writable` vector
    /// does not match the number of players.
    pub fn add_players(mut self, new_player_list: Vec<PlayerBuilder>) -> BigBang {
        for player in new_player_list {
            self = self.add_player(player);
        }
        self
    }

    /// Updates the fog to a new visibility radius.
    /// This is used to grant players visibility into the fog when
    /// they are competing against other players and they create
    /// cells outside of their own writiable regions.
    pub fn fog_radius(mut self, new_radius : usize) -> BigBang {
        self.fog_radius = new_radius;
        self
    }

    /// "Gives life to the universe and the first moment of time."
    /// Creates a Universe which can then CGoL process generations.
    /// 
    /// # Errors
    /// 
    /// - if `width` or `height` are not positive, or if `width` is not a multiple of 64.
    /// - if `fog_radius` is not positive.
    /// - if `history` is not positive.
    pub fn birth(&self) -> Result<Universe, UniverseError> {
        let universe = Universe::new(
            self.width,
            self.height,
            self.is_server,
            self.history,
            self.num_players,      // number of players in the game (player numbers are 0-based)
            self.player_writable.clone(), // writable region (indexed by player_id)
            self.fog_radius,       // fog radius provides visiblity outside of writable regions
        );
        universe
    }
}


/// Represents a wrapping universe in Conway's game of life.
pub struct Universe {
    width:           usize,
    height:          usize,
    width_in_words:  usize,                     // width in u64 elements, _not_ width in cells!
    generation:      usize,                     // current generation (1-based)
    num_players:     usize,                     // number of players in the game (player numbers are 0-based)
    state_index:     usize,                     // index of GenState for current generation within gen_states
    gen_states:      Vec<GenState>,             // circular buffer of generational states
    player_writable: Vec<Region>,               // writable region (indexed by player_id)
    fog_radius:      usize,
    fog_circle:      BitGrid,
}


// Describes the state of the universe for a particular generation
// This includes any cells alive, known, and each player's own gen states
// for this current session
#[derive(Debug, Clone, PartialEq)]
pub struct GenState {
    gen_or_none:   Option<usize>,        // Some(generation number) (redundant info); if None, this is an unused buffer
    cells:         BitGrid,              // 1 = cell is known to be Alive
    wall_cells:    BitGrid,              // 1 = is a wall cell (should this just be fixed for the universe?)
    known:         BitGrid,              // 1 = cell is known (always 1 if this is server)
    player_states: Vec<PlayerGenState>,  // player-specific info (indexed by player_id)
}

#[derive(Debug, Clone)]
pub struct GenStateDiff {
    pub gen0:    usize,
    pub gen1:    usize,
    pub pattern: Pattern,
}

#[derive(Debug, Clone, PartialEq)]
struct PlayerGenState {
    cells:     BitGrid,   // cells belonging to this player (if 1 here, must be 1 in GenState cells)
    fog:       BitGrid,   // cells that are currently invisible to the player
}


#[derive(Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Debug)]
pub enum CellState {
    Dead,
    Alive(Option<usize>),    // Some(player_number) or alive but not belonging to any player
    Wall,
    Fog,
}

impl CellState {
    /// Convert this `CellState` to a `char`. When the state is `Alive(None)` or `Dead`, this will
    /// match what would be found in a .rle file. `Wall`, `Alive(Some(player_id))`, and `Fog` are
    /// unsupported in vanilla CGoL, and thus are not part of the [RLE
    /// specification](http://www.conwaylife.com/wiki/Run_Length_Encoded).
    /// 
    /// # Panics
    /// 
    /// Panics if `player_id` is not less than 23, since we map IDs 0 through 22 to uppercase
    /// letters A through V. W is not usable since it represents a wall cell.
    pub fn to_char(self) -> char {
        match self {
            CellState::Alive(Some(player_id)) => {
                if player_id >= 23 {
                    panic!("Player IDs must be less than 23 to be converted to chars");
                }
                char::from_u32(player_id as u32 + 65).unwrap()
            }
            CellState::Alive(None) => 'o',
            CellState::Dead        => 'b',
            CellState::Wall        => 'W',
            CellState::Fog         => '?',
        }
    }

    // TODO: doc comment
    pub fn from_char(ch: char) -> Option<Self> {
        match ch {
            'o' => Some(CellState::Alive(None)),
            'b' => Some(CellState::Dead),
            'W' => Some(CellState::Wall),
            '?' => Some(CellState::Fog),
            'A'..='V' => {
                Some(CellState::Alive(Some(u32::from(ch) as usize - 65)))
            }
            _ => {
                None
            }
        }
    }
}


impl GenState {
    /// Sets the state of a cell, with minimal checking.  It doesn't support setting
    /// `CellState::Fog`.
    /// 
    /// # Panics
    /// 
    /// Panics if an attempt is made to set an unknown cell.
    pub fn set_unchecked(&mut self, col: usize, row: usize, new_state: CellState) {
        let word_col = col/64;
        let shift = 63 - (col & (64 - 1)); // translate literal col (ex: 134) to bit index in word_col
        let mask  = 1 << shift;     // cell to set

        // panic if not known
        let known_cell_word = self.known[row][word_col];
        if known_cell_word & mask == 0 {
            panic!("Tried to set unknown cell at ({}, {})", col, row);
        }

        // clear all player cell bits, so that this cell is unowned by any player (we'll set
        // ownership further down)
        {
            for player_id in 0 .. self.player_states.len() {
                let ref mut grid = self.player_states[player_id].cells;
                grid.modify_bits_in_word(row, word_col, mask, BitOperation::Clear);
            }
        }

        let cells = &mut self.cells;
        let walls  = &mut self.wall_cells;
        match new_state {
            CellState::Dead => {
                cells.modify_bits_in_word(row, word_col, mask, BitOperation::Clear);
                walls.modify_bits_in_word(row, word_col, mask, BitOperation::Clear);
            }
            CellState::Alive(opt_player_id) => {
                cells.modify_bits_in_word(row, word_col, mask, BitOperation::Set);
                walls.modify_bits_in_word(row, word_col, mask, BitOperation::Clear);

                if let Some(player_id) = opt_player_id {
                    let ref mut player = self.player_states[player_id];
                    player.cells.modify_bits_in_word(row, word_col, mask, BitOperation::Set);
                    player.fog.modify_bits_in_word(row, word_col, mask, BitOperation::Clear);
                }
            }
            CellState::Wall => {
                cells.modify_bits_in_word(row, word_col, mask, BitOperation::Clear);
                walls.modify_bits_in_word(row, word_col, mask, BitOperation::Set);
            }
            _ => unimplemented!()
        }
    }

    /// Copies from `src` BitGrid to this GenState as the player specified by `opt_player_id`,
    /// unless `opt_player_id` is `None`. This is an "or" operation, so any existing alive cells
    /// are retained, though they may change ownership.  Walls, however, are preserved. Fog is
    /// cleared on a cell-by-cell basis, rather than using fog radius.
    ///
    /// IMPORTANT: dst_region should not extend beyond GenState, nor beyond player's writable
    /// region. Caller may ensure this using Region::intersection.
    ///
    /// The top-left cell (that is, the cell at `(0,0)`) in `src` gets written to `(dst_region.top(),
    /// dst_region.left())` in `dst`.
    pub fn copy_from_bit_grid(&mut self, src: &BitGrid, dst_region: Region, opt_player_id: Option<usize>) {

        BitGrid::copy(src, &mut self.cells, dst_region);

        if let Some(player_id) = opt_player_id {

            BitGrid::copy(src, &mut self.player_states[player_id].cells, dst_region);

            for row in dst_region.top()..=dst_region.bottom() {
                let row = row as usize;

                // This actually can operate on cells to the left and right of dst_region, but that
                // shouldn't matter, since it's enforcing an invariant that should already be true.
                for word_col in (dst_region.left()/64) ..= (dst_region.right()/64) {
                    let word_col = word_col as usize;

                    // for each wall bit that's 1, clear it in player's cells
                    self.player_states[player_id].cells[row][word_col] &= !self.wall_cells[row][word_col];
                    // for each player cell bit that's 1, clear it in player's fog
                    self.player_states[player_id].fog[row][word_col] &= !self.player_states[player_id].cells[row][word_col];
                }
            }
        }

        // on the rows in dst_region, for each wall bit that's 1, clear it in dst.cells
        for row in dst_region.top()..=dst_region.bottom() {
            let row = row as usize;

            for word_col in (dst_region.left()/64) ..= (dst_region.right()/64) {
                let word_col = word_col as usize;

                self.cells[row][word_col] &= !self.wall_cells[row][word_col];
            }
        }
    }

    /// Creates a "diff" RLE pattern (contained within GenStateDiff) showing the changes present in
    /// `new`, using `self` as a base (that is, `self` is assumed to be "old"). If `visibility` is
    /// not `None`, only the changes visible to specified player will be recorded.
    ///
    /// The `gen0` field of the result will be equal to `self.gen_or_none.unwrap()` and the `gen1`
    /// field will be equal to `new.gen_or_none.unwrap()`.
    ///
    /// When `visibility` is `Some(player_id)`, then for all possible `gs0` and `gs1` where
    /// `player_id` is valid and dimensions match, the following should be true:
    ///
    ///  ```no_run
    ///  # use conway::universe::GenState;
    ///  # fn do_eet(gs0: GenState, gs1: GenState, mut new_gs: GenState, visibility: Option<usize>) {
    ///  let gsdiff = gs0.diff(&gs1, visibility);
    ///  gsdiff.pattern.to_grid(&mut new_gs, visibility).unwrap();
    ///  assert_eq!(new_gs, gs1);
    ///  # }
    ///  ```
    ///
    /// Panics:
    ///
    /// * This will panic if either `self.gen_or_none` or `new.gen_or_none` is `None`.
    /// * This will panic if the lengths of the `player_states` vectors do not match.
    /// * This will panic if the dimensions of the grids do not match.
    pub fn diff(&self, new: &GenState, visibility: Option<usize>) -> GenStateDiff {
        if self.height() != new.height() || self.width() != new.width() {
            panic!("Dimensions do not match: {}x{} vs {}x{}", self.width(), self.height(), new.width(), new.height());
        }

        let self_gen = self.gen_or_none.unwrap();
        let new_gen = new.gen_or_none.unwrap();

        if self.player_states.len() != new.player_states.len() {
            panic!("Player state vectors do not match");
        }

        let pair = GenStatePair {
            gen_state0: &self,
            gen_state1: &new,
        };
        let pattern = pair.to_pattern(visibility);

        GenStateDiff {
            gen0: self_gen,
            gen1: new_gen,
            pattern,
        }
    }

    /// Zeroes out all bit grids. Note: this means fog is cleared for all players.
    pub fn clear(&mut self) {
        let region = Region::new(0, 0, self.width(), self.height());
        self.cells.modify_region(region, BitOperation::Clear);
        self.known.modify_region(region, BitOperation::Clear);
        self.wall_cells.modify_region(region, BitOperation::Clear);

        for player_id in 0..self.player_states.len() {
            let p = &mut self.player_states[player_id];
            p.cells.modify_region(region, BitOperation::Clear);
            p.fog.modify_region(region, BitOperation::Clear);
        }
    }

    pub fn copy(&self, dest: &mut GenState) {
        let region = Region::new(0, 0, self.width(), self.height());
        BitGrid::copy(&self.cells, &mut dest.cells, region);
        BitGrid::copy(&self.known, &mut dest.known, region);
        BitGrid::copy(&self.wall_cells, &mut dest.wall_cells, region);

        for player_id in 0..dest.player_states.len() {
            BitGrid::copy(&self.player_states[player_id].cells, &mut dest.player_states[player_id].cells, region);
            BitGrid::copy(&self.player_states[player_id].fog, &mut dest.player_states[player_id].fog, region);
        }
    }
}

impl CharGrid for GenState {
    /// Width in cells
    fn width(&self) -> usize {
        self.cells.width()
    }

    /// Height in cells
    fn height(&self) -> usize {
        self.cells.height()
    }

    #[inline]
    fn write_at_position(&mut self, col: usize, row: usize, ch: char, visibility: Option<usize>) {
        if !GenState::is_valid(ch) {
            panic!(format!("char {:?} is invalid for this CharGrid", ch));
        }
        let word_col = col/64;
        let shift = 63 - (col & (64 - 1));
        // cells
        match ch {
            'b' | 'W' | '?' => {
                self.cells[row][word_col] &= !(1 << shift)
            }
            'o' | 'A'..='V' => {
                self.cells[row][word_col] |=   1 << shift
            }
            _ => unreachable!()
        }
        // wall cells
        match ch {
            'W' => {
                self.wall_cells[row][word_col] |=   1 << shift
            }
            'b' | 'o' | 'A'..='V' | '?' => {
                self.wall_cells[row][word_col] &= !(1 << shift)
            }
            _ => unreachable!()
        }
        // player_states
        if ch == '?' {
            if visibility.is_none() {
                // I expect that only clients will read a pattern containing fog, and clients will
                // never have visibility set to None.
                panic!("cannot write fog when no player_id is specified");
            }
            let player_id = visibility.unwrap();
            // only set fog bit for specified player
            self.player_states[player_id].fog[row][word_col] |= 1 << shift;
        } else {
            self.known[row][word_col] |= 1 << shift;    // known
            if let Some(player_id) = visibility {
                // only clear fog bit for specified player
                self.player_states[player_id].fog[row][word_col] &= !(1 << shift);
            } else {
                // clear fog bit for all players
                for i in 0 .. self.player_states.len() {
                    self.player_states[i].fog[row][word_col] &= !(1 << shift);
                }
            }
            // clear all player's cells
            for i in 0 .. self.player_states.len() {
                self.player_states[i].cells[row][word_col] &= !(1 << shift);
            }
            // if 'A'..='V', set that player's cells
            if ch >= 'A' && ch <= 'V' {
                let p_id = ch as usize - 'A' as usize;
                self.player_states[p_id].cells[row][word_col] |= 1 << shift; // can panic if p_id out of range
            }
        }
    }

    #[inline]
    fn is_valid(ch: char) -> bool {
        match ch {
            'o' | 'b' | 'A'..='W' | '?' => true,
            NO_OP_CHAR => true,
            _ => false
        }
    }

    /// Given a starting cell at `(col, row)`, get the character at that cell, and the number of
    /// contiguous identical cells considering only this cell and the cells to the right of it.
    /// This is intended for exporting to RLE.
    ///
    /// The `visibility` parameter, if not `None`, is used to generate a run as observed by a
    /// particular player.
    ///
    /// # Returns
    ///
    /// `(run_length, ch)`
    ///
    /// # Panics
    ///
    /// This function will panic if `col`, `row`, or `visibility` (`Some(player_id)`) are out of bounds.
    fn get_run(&self, col: usize, row: usize, visibility: Option<usize>) -> (usize, char) {
        let mut min_run = self.width() - col;

        let (known_run, known_ch) = self.known.get_run(col, row, None);
        if known_run < min_run { min_run = known_run; }
        if known_ch == 'b' {
            return (min_run, CellState::Fog.to_char());
        }

        if let Some(player_id) = visibility {
            let (fog_run, fog_ch) = self.player_states[player_id].fog.get_run(col, row, None);
            if fog_run < min_run { min_run = fog_run; }
            if fog_ch == 'o' {
                return (min_run, CellState::Fog.to_char());
            }
        }

        let (cell_run, cell_ch) = self.cells.get_run(col, row, None);
        if cell_run < min_run { min_run = cell_run; }

        let (wall_run, wall_ch) = self.wall_cells.get_run(col, row, None);
        if wall_run < min_run { min_run = wall_run; }

        if cell_ch == 'o' {
            for player_id in 0 .. self.player_states.len() {
                let (player_cell_run, player_cell_ch) = self.player_states[player_id].cells.get_run(col, row, None);
                if player_cell_run < min_run { min_run = player_cell_run; }
                if player_cell_ch == 'o' {
                    let owned_ch = CellState::Alive(Some(player_id)).to_char();
                    return (min_run, owned_ch);
                }
            }
            return (min_run, CellState::Alive(None).to_char());
        }
        if wall_ch == 'o' {
            return (min_run, CellState::Wall.to_char());
        } else {
            return (min_run, CellState::Dead.to_char());
        }
    }
}


/// This internal struct is only needed so we can implement CharGrid::to_pattern. It's a little silly...
struct GenStatePair<'a,'b> {
    gen_state0: &'a GenState,
    gen_state1: &'b GenState,
}


impl<'a,'b> CharGrid for GenStatePair<'a,'b> {
    /// Width in cells
    fn width(&self) -> usize {
        self.gen_state0.width()
    }

    /// Height in cells
    fn height(&self) -> usize {
        self.gen_state0.height()
    }

    fn write_at_position(&mut self, _col: usize, _row: usize, _ch: char, _visibility: Option<usize>) {
        unimplemented!("This is a read-only struct!");
    }

    /// Is `ch` a valid character?
    fn is_valid(ch: char) -> bool {
        if ch == NO_OP_CHAR {
            return true;
        }
        GenState::is_valid(ch)
    }

    /// Given a starting cell at `(col, row)`, get the character at that cell, and the number of
    /// contiguous identical cells considering only this cell and the cells to the right of it.
    /// This is intended for exporting to RLE.
    ///
    /// The `visibility` parameter, if not `None`, is used to generate a run as observed by a
    /// particular player.
    ///
    /// # Returns
    ///
    /// `(run_length, ch)`
    ///
    /// # Panics
    ///
    /// This function will panic if `col`, `row`, or `visibility` (`Some(player_id)`) are out of bounds.
    fn get_run(&self, mut col: usize, row: usize, visibility: Option<usize>) -> (usize, char) {
        let (run0, ch0) = self.gen_state0.get_run(col, row, visibility);
        let (run1, ch1) = self.gen_state1.get_run(col, row, visibility);
        let ch;
        if ch0 == ch1 {
            ch = NO_OP_CHAR;  // no change
        } else {
            ch = ch1;         // change here; return the new character
        }
        let mut run = cmp::min(run0, run1);
        let mut total_run = run;
        if ch == NO_OP_CHAR {
            loop {
                col += run;
                if col >= self.width() {
                    break;
                }
                let (run0, ch0) = self.gen_state0.get_run(col, row, visibility);
                let (run1, ch1) = self.gen_state1.get_run(col, row, visibility);
                if ch0 != ch1 {
                    break;
                }
                run = cmp::min(run0, run1);
                total_run += run;
            }
        }
        (total_run, ch)
    }
}


impl fmt::Display for Universe {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let cells = &self.gen_states[self.state_index].cells;
        let wall  = &self.gen_states[self.state_index].wall_cells;
        let known = &self.gen_states[self.state_index].known;
        for row_idx in 0 .. self.height {
            for col_idx in 0 .. self.width_in_words {
                let cell_cen  = cells[row_idx][col_idx];
                let wall_cen  = wall [row_idx][col_idx];
                let known_cen = known[row_idx][col_idx];
                let mut s = String::with_capacity(64);
                for shift in (0..64).rev() {
                    if (known_cen>>shift)&1 == 0 {
                        s.push('?');
                    } else if (cell_cen>>shift)&1 == 1 {
                        let mut is_player = false;
                        for player_id in 0 .. self.num_players {
                            let player_word = self.gen_states[self.state_index].player_states[player_id].cells[row_idx][col_idx];
                            if (player_word>>shift)&1 == 1 {
                                s.push(char::from_u32(player_id as u32 + 65).unwrap());
                                is_player = true;
                                break;
                            }
                        }
                        if !is_player { s.push('*'); }
                    } else if (wall_cen>>shift)&1 == 1 {
                        s.push('W');
                    } else {
                        s.push(' ');
                    }
                }
                write!(f, "{}", s)?;
            }
            write!(f, "\n")?;
        }
        Ok(())
    }
}


impl Universe {

    /// Gets a `CellState` enum for cell at (`col`, `row`).
    /// 
    /// # Panics
    /// 
    /// Panics if `row` or `col` are out of range.
    pub fn get_cell_state(&mut self, col: usize, row: usize, opt_player_id: Option<usize>) -> CellState {
        let gen_state = &mut self.gen_states[self.state_index];
        let word_col = col/64;
        let shift = 63 - (col & (64 - 1)); // translate literal col (ex: 134) to bit index in word_col
        let mask  = 1 << shift;     // cell to set

        if let Some(player_id) = opt_player_id {
            let cell = (gen_state.player_states[player_id].cells[row][word_col] & mask) >> shift;
            if cell == 1 {CellState::Alive(opt_player_id)} else {CellState::Dead}
        }
        else {
            let cell = (gen_state.cells[row][word_col] & mask) >> shift;
            if cell == 1 {CellState::Alive(None)} else {CellState::Dead}
        }
    }


    /// Sets the state of a cell in the latest generation, with minimal checking.  It doesn't
    /// support setting `CellState::Fog`.
    /// 
    /// # Panics
    /// 
    /// Panics if an attempt is made to set an unknown cell.
    pub fn set_unchecked(&mut self, col: usize, row: usize, new_state: CellState) {
        self.gen_states[self.state_index].set_unchecked(col, row, new_state)
    }


    /// Checked set - check for:
    /// * current cell state (can't change wall)
    /// * player writable region
    /// * fog
    /// * if current cell is alive, player_id matches player_id argument
    ///
    /// If any of the above checks fail, do nothing.
    ///
    /// # Panics
    ///
    /// Panic if player_id inside CellState does not match player_id argument.
    pub fn set(&mut self, col: usize, row: usize, new_state: CellState, player_id: usize) {

        {
            let gen_state = &mut self.gen_states[self.state_index];
            let word_col = col/64;
            let shift = 63 - (col & (64 - 1));
            let mask  = 1 << shift;     // bit to set for cell represented by (row,col)

            let cells = &mut gen_state.cells;
            let wall  = &mut gen_state.wall_cells;
            let cells_word = cells[row][word_col];
            let walls_word = wall [row][word_col];

            if walls_word & mask > 0 {
                return;
            }

            if !self.player_writable[player_id].contains(col as isize, row as isize) { return;
            }

            if gen_state.player_states[player_id].fog[row][word_col] & mask > 0 {
                return;
            }

            // If the current cell is alive but not owned by this player, do nothing
            if cells_word & mask > 0 && gen_state.player_states[player_id].cells[row][word_col] & mask == 0 {
                return;
            }

            if let CellState::Alive(Some(new_state_player_id)) = new_state {
                if new_state_player_id != player_id {
                    panic!("A player cannot set the cell state of another player");
                }
            }
        }

        self.set_unchecked(col, row, new_state)
    }


    /// Switches any non-dead state to CellState::Dead.
    /// Switches CellState::Dead to CellState::Alive(opt_player_id) and clears fog for that player,
    /// if any.
    ///
    /// This operation works in three steps:
    ///  1. Toggle alive/dead cell in the current generation state cell grid
    ///  2. Clear all players' cell
    ///  3. If general cell transitioned Dead->Alive, then set requested player's cell
    ///
    /// The new value of the cell is returned.
    pub fn toggle_unchecked(&mut self, col: usize, row: usize, opt_player_id: Option<usize>) -> CellState {
        let word_col = col/64;
        let shift = 63 - (col & (64 - 1));
        let mask = 1 << shift;

        let word =
        {
            let cells = &mut self.gen_states[self.state_index].cells;
            cells.modify_bits_in_word(row, word_col, mask, BitOperation::Toggle);
            cells[row][word_col]
        };

        // Cell transitioned Dead -> Alive 
        let next_cell = (word & mask) > 0;

        // clear all player cell bits
        for player_id in 0 .. self.num_players {
            let ref mut player_cells = self.gen_states[self.state_index].player_states[player_id].cells;
            player_cells.modify_bits_in_word(row, word_col, mask, BitOperation::Clear);
        }

        if next_cell {
            // set this player's cell bit, if needed, and clear fog
            if let Some(player_id) = opt_player_id {
                let ref mut player = self.gen_states[self.state_index].player_states[player_id];
                player.cells.modify_bits_in_word(row, word_col, mask, BitOperation::Set);
                player.fog.modify_bits_in_word(row, word_col, mask, BitOperation::Clear);
            }

            CellState::Alive(opt_player_id)
        } else {
            CellState::Dead
        }
    }


    /// Checked toggle - switch between CellState::Alive and CellState::Dead.
    /// 
    /// # Errors
    /// 
    /// It is an error to toggle outside player's writable area, or to toggle a wall or an unknown cell.
    pub fn toggle(&mut self, col: usize, row: usize, player_id: usize) -> Result<CellState, ()> {
        if !self.player_writable[player_id].contains(col as isize, row as isize) {
            return Err(());
        }

        let word_col = col/64;
        let shift = 63 - (col & (64 - 1));
        {
            let wall  = &self.gen_states[self.state_index].wall_cells;
            let known = &self.gen_states[self.state_index].known;
            if (wall[row][word_col] >> shift) & 1 == 1 || (known[row][word_col] >> shift) & 1 == 0 {
                return Err(());
            }
        }
        Ok(self.toggle_unchecked(col, row, Some(player_id)))
    }


    /// Instantiate a new blank universe with the given width and height, in cells.
    /// The universe is at generation 1.
    /// 
    /// **Note**: it is easier to use `BigBang` to build a `Universe`, as that has default values
    /// that can be overridden as needed.
    pub fn new(width:           usize,
               height:          usize,
               is_server:       bool,
               history:         usize,
               num_players:     usize,
               player_writable: Vec<Region>,
               fog_radius:      usize) -> Result<Universe, UniverseError> {
        if height == 0 {
            return Err("Height must be positive".to_owned());
        }

        let width_in_words = width/64;
        if width % 64 != 0 {
            return Err("Width must be a multiple of 64".to_owned());
        } else if width == 0 {
            return Err("Width must be positive".to_owned());
        }

        if history == 0 {
            return Err("History must be positive".to_owned());
        }

        if fog_radius == 0 {
            return Err("Fog radius must be positive".to_owned());
        }

        // Initialize all generational states with the default appropriate bitgrids
        let mut gen_states = Vec::new();
        for i in 0 .. history {
            let mut player_states = Vec::new();
            for player_id in 0 .. num_players {

                let mut pgs = PlayerGenState {
                    cells:     BitGrid::new(width_in_words, height),
                    fog:       BitGrid::new(width_in_words, height),
                };

                // unless writable region, the whole grid is player fog
                pgs.fog.modify_region(Region::new(0, 0, width, height), BitOperation::Set);

                // clear player fog on writable regions
                pgs.fog.modify_region(player_writable[player_id], BitOperation::Clear);

                player_states.push(pgs);
            }

            // Known cells describe what the current operative (player, server)
            // visibility reaches. For example, a Server has total visibility as
            // it needs to know all.
            let mut known = BitGrid::new(width_in_words, height);
            
            if is_server && i == 0 {
                // could use modify_region but its much cheaper this way
                for y in 0 .. height {
                    for x in 0 .. width_in_words {
                        known[y][x] = u64::max_value();   // if server, all cells are known
                    }
                }
            }

            gen_states.push(GenState {
                gen_or_none:   if i == 0 { Some(1) } else { None },
                cells:         BitGrid::new(width_in_words, height),
                wall_cells:    BitGrid::new(width_in_words, height),
                known:         known,
                player_states: player_states,
            });
        }

        let mut uni = Universe {
            width:           width,
            height:          height,
            width_in_words:  width_in_words,
            generation:      1,
            num_players:     num_players,
            state_index:     0,
            gen_states:      gen_states,
            player_writable: player_writable,
            // TODO: it's not very rusty to have uninitialized stuff (use Option<FogInfo> instead)
            fog_radius:      fog_radius,      // uninitialized
            fog_circle:      BitGrid(vec![]), // uninitialized
        };
        uni.generate_fog_circle_bitmap();
        Ok(uni)
    }


    /// Pre-computes a "fog circle" bitmap of given cell radius to be saved to the `Universe`
    /// struct. This bitmap is used for clearing fog around a player's cells.
    ///
    /// The bitmap has 0 bits inside the circle radius, and 1 bits elsewhere. The bitmap has
    /// width and height such that the circle's height exactly fits, and the left edge of the
    /// circle touches the left edge of the bitmap. Therefore, before masking out the fog, it
    /// must be shifted up and to the left by `fog_radius - 1` cells.
    ///
    /// Notable `fog_radius` values:
    /// * 1: This does not clear any fog around the cell
    /// * 2: This clears the cell and its neighbors
    /// * 4: Smallest radius at which the cleared fog region is not a square
    /// * 8: Smallest radius at which the cleared fog region is neither square nor octagon
    fn generate_fog_circle_bitmap(&mut self) {
        let fog_radius = self.fog_radius;
        let height = 2*fog_radius - 1;
        let word_width = (height - 1) / 64 + 1;
        self.fog_circle = BitGrid::new(word_width, height);

        // Parts outside the circle must be 1, so initialize with 1 first, then draw the
        // filled-in circle, containing 0 bits.
        for y in 0 .. height {
            for x in 0 .. word_width {
                self.fog_circle[y][x] = u64::max_value();
            }
        }

        // calculate the center bit coordinates
        let center_x = (fog_radius - 1) as isize;
        let center_y = (fog_radius - 1) as isize;
        // algebra!
        for y in 0 .. height {
            for bit_x in 0 .. word_width*64 {
                let shift = 63 - (bit_x & 63);
                let mask = 1<<shift;
                // calculate x_delta and y_delta
                let x_delta = isize::abs(center_x - bit_x as isize) as usize;
                let y_delta = isize::abs(center_y - y as isize) as usize;
                if x_delta*x_delta + y_delta*y_delta < fog_radius*fog_radius {
                    self.fog_circle[y][bit_x/64] &= !mask;
                }
            }
        }
    }


    /// Get the latest generation number (1-based).
    pub fn latest_gen(&self) -> usize {
        assert!(self.generation != 0);
        self.generation
    }

    fn next_single_gen(nw: u64, n: u64, ne: u64, w: u64, center: u64, e: u64, sw: u64, s: u64, se: u64) -> u64 {
        let a  = (nw     << 63) | (n      >>  1);
        let b  =  n;
        let c  = (n      <<  1) | (ne     >> 63);
        let d  = (w      << 63) | (center >> 1);
        let y6 = center;
        let e  = (center <<  1) | (e      >> 63);
        let f  = (sw     << 63) | (s      >>  1);
        let g  =  s;
        let h  = (s      <<  1) | (se     >> 63);

        // full adder #1
        let b_xor_c = b^c;
        let y1 = (a & b_xor_c) | (b & c);
        let y2 = a ^ b_xor_c;

        // full adder #2
        let e_xor_f = e^f;
        let c2 = (d & e_xor_f) | (e & f);
        let s2 = d ^ e_xor_f;

        // half adder #1
        let c3 = g & h;
        let s3 = g ^ h;

        // half adder #2
        let c4 = s2 & s3;
        let y5 = s2 ^ s3;

        // full adder #3
        let c2_xor_c3 = c2 ^ c3;
        let y3 = (c4 & c2_xor_c3) | (c2 & c3);
        let y4 = c4 ^ c2_xor_c3;

        let int1 = !y3 & !y4;
        !y1&y6&(y2&int1&y5 | y4&!y5) | y1&int1&(!y2&(y5 | y6) | y2&!y5) | !y1&y4&(y2^y5)
    }

    /*
     * A B C
     * D   E
     * F G H
     */
    // a cell is 0 if itself or any of its neighbors are 0
    fn contagious_zero(nw: u64, n: u64, ne: u64, w: u64, center: u64, e: u64, sw: u64, s: u64, se: u64) -> u64 {
        let a  = (nw     << 63) | (n      >>  1);
        let b  =  n;
        let c  = (n      <<  1) | (ne     >> 63);
        let d  = (w      << 63) | (center >> 1);
        let e  = (center <<  1) | (e      >> 63);
        let f  = (sw     << 63) | (s      >>  1);
        let g  =  s;
        let h  = (s      <<  1) | (se     >> 63);
        a & b & c & d & center & e & f & g & h
    }


    // a cell is 1 if itself or any of its neighbors are 1
    fn contagious_one(nw: u64, n: u64, ne: u64, w: u64, center: u64, e: u64, sw: u64, s: u64, se: u64) -> u64 {
        let a  = (nw     << 63) | (n      >>  1);
        let b  =  n;
        let c  = (n      <<  1) | (ne     >> 63);
        let d  = (w      << 63) | (center >> 1);
        let e  = (center <<  1) | (e      >> 63);
        let f  = (sw     << 63) | (s      >>  1);
        let g  =  s;
        let h  = (s      <<  1) | (se     >> 63);
        a | b | c | d | center | e | f | g | h
    }


    /// Compute the next generation. Returns the new latest generation number.
    pub fn next(&mut self) -> usize {
        // get the buffers and buffers_next
        assert!(self.gen_states[self.state_index].gen_or_none.unwrap() == self.generation);
        let history = self.gen_states.len();
        let next_state_index = (self.state_index + 1) % history;

        let (gen_state, gen_state_next) = if self.state_index < next_state_index {
            let (p0, p1) = self.gen_states.split_at_mut(next_state_index);
            (&p0[next_state_index - 1], &mut p1[0])
        } else {
            // self.state_index == history-1 and next_state_index == 0
            let (p0, p1) = self.gen_states.split_at_mut(next_state_index + 1);
            (&p1[history - 2], &mut p0[0])
        };

        {
            let cells      = &gen_state.cells;
            let wall       = &gen_state.wall_cells;
            let known      = &gen_state.known;
            let cells_next = &mut gen_state_next.cells;
            let wall_next  = &mut gen_state_next.wall_cells;
            let known_next = &mut gen_state_next.known;

            // Copy fog over to next generation
            for row_idx in 0 .. self.height {
                for player_id in 0 .. self.num_players {
                    gen_state_next.player_states[player_id].fog[row_idx].copy_from_slice(&gen_state.player_states[player_id].fog[row_idx]);
                }
            }

            for row_idx in 0 .. self.height {
                let n_row_idx = (row_idx + self.height - 1) % self.height;
                let s_row_idx = (row_idx + 1) % self.height;
                let cells_row_n = &cells[n_row_idx];
                let cells_row_c = &cells[ row_idx ];
                let cells_row_s = &cells[s_row_idx];
                let wall_row_c  = &wall[ row_idx ];
                let known_row_n = &known[n_row_idx];
                let known_row_c = &known[ row_idx ];
                let known_row_s = &known[s_row_idx];

                // These will be shifted over at the beginning of the loop
                let mut cells_nw;
                let mut cells_w;
                let mut cells_sw;
                let mut cells_n   = cells_row_n[self.width_in_words - 1];
                let mut cells_cen = cells_row_c[self.width_in_words - 1];
                let mut cells_s   = cells_row_s[self.width_in_words - 1];
                let mut cells_ne  = cells_row_n[0];
                let mut cells_e   = cells_row_c[0];
                let mut cells_se  = cells_row_s[0];
                let mut known_nw;
                let mut known_w;
                let mut known_sw;
                let mut known_n   = known_row_n[self.width_in_words - 1];
                let mut known_cen = known_row_c[self.width_in_words - 1];
                let mut known_s   = known_row_s[self.width_in_words - 1];
                let mut known_ne  = known_row_n[0];
                let mut known_e   = known_row_c[0];
                let mut known_se  = known_row_s[0];

                for col_idx in 0 .. self.width_in_words {
                    // shift over
                    cells_nw  = cells_n;
                    cells_n   = cells_ne;
                    cells_w   = cells_cen;
                    cells_cen = cells_e;
                    cells_sw  = cells_s;
                    cells_s   = cells_se;
                    cells_ne  = cells_row_n[(col_idx + 1) % self.width_in_words];
                    cells_e   = cells_row_c[(col_idx + 1) % self.width_in_words];
                    cells_se  = cells_row_s[(col_idx + 1) % self.width_in_words];
                    known_nw  = known_n;
                    known_n   = known_ne;
                    known_w   = known_cen;
                    known_cen = known_e;
                    known_sw  = known_s;
                    known_s   = known_se;
                    known_ne  = known_row_n[(col_idx + 1) % self.width_in_words];
                    known_e   = known_row_c[(col_idx + 1) % self.width_in_words];
                    known_se  = known_row_s[(col_idx + 1) % self.width_in_words];

                    // apply BitGrid changes
                    let mut cells_cen_next = Universe::next_single_gen(cells_nw, cells_n, cells_ne, cells_w, cells_cen, cells_e, cells_sw, cells_s, cells_se);

                    // any known cells with at least one unknown neighbor will become unknown in
                    // the next generation
                    known_next[row_idx][col_idx] = Universe::contagious_zero(known_nw, known_n, known_ne, known_w, known_cen, known_e, known_sw, known_s, known_se);

                    cells_cen_next &= known_next[row_idx][col_idx];
                    cells_cen_next &= !wall_row_c[col_idx];

                    // assign to the u64 element in the next generation
                    cells_next[row_idx][col_idx] = cells_cen_next;

                    let mut in_multiple: u64 = 0;
                    let mut seen_before: u64 = 0;
                    for player_id in 0 .. self.num_players {
                        // Any unknown cell with 
                        //
                        // A cell which would have belonged to 2+ players in the next
                        // generation will belong to no one. These are unowned cells.
                        //
                        // Unowned cells follow the same rules of life.
                        //
                        // Any unowned cells are influenced by their neighbors, and if players,
                        // can be acquired by the player, just as long as no two players are
                        // fighting over those cells
                        let player_cell_next =
                            Universe::contagious_one(
                                gen_state.player_states[player_id].cells[n_row_idx][(col_idx + self.width_in_words - 1) % self.width_in_words],
                                gen_state.player_states[player_id].cells[n_row_idx][col_idx],
                                gen_state.player_states[player_id].cells[n_row_idx][(col_idx + 1) % self.width_in_words],
                                gen_state.player_states[player_id].cells[ row_idx ][(col_idx + self.width_in_words - 1) % self.width_in_words],
                                gen_state.player_states[player_id].cells[ row_idx ][col_idx],
                                gen_state.player_states[player_id].cells[ row_idx ][(col_idx + 1) % self.width_in_words],
                                gen_state.player_states[player_id].cells[s_row_idx][(col_idx + self.width_in_words - 1) % self.width_in_words],
                                gen_state.player_states[player_id].cells[s_row_idx][col_idx],
                                gen_state.player_states[player_id].cells[s_row_idx][(col_idx + 1) % self.width_in_words]
                            ) & cells_cen_next;
                        in_multiple |= player_cell_next & seen_before;
                        seen_before |= player_cell_next;
                        gen_state_next.player_states[player_id].cells[row_idx][col_idx] = player_cell_next;
                    }
                    for player_id in 0 .. self.num_players {
                        let cell_cur = gen_state.player_states[player_id].cells[row_idx][col_idx];
                        let mut cell_next = gen_state_next.player_states[player_id].cells[row_idx][col_idx];
                        cell_next &= !in_multiple; // if a cell would have belonged to multiple players, it belongs to none
                        gen_state_next.player_states[player_id].cells[row_idx][col_idx] = cell_next;

                        // clear fog for all cells that turned on in this generation
                        Universe::clear_fog(&mut gen_state_next.player_states[player_id].fog, &self.fog_circle, self.fog_radius, self.width, self.height, row_idx, col_idx, cell_next & !cell_cur);
                    }
                }

                // copy wall to wall_next
                wall_next[row_idx].copy_from_slice(wall_row_c);
            }
        }

        // increment generation in appropriate places
        self.generation += 1;
        self.state_index = next_state_index;
        gen_state_next.gen_or_none = Some(self.generation);
        self.generation
    }


    /// Clears the fog for the specified bits in the 64-bit word at `center_row_idx` and
    /// `center_col_idx` using the fog circle (see `generate_fog_circle_bitmap` documentation for
    /// more on this).
    //TODO: unit test with fog_radiuses above and below 64
    fn clear_fog(player_fog:     &mut BitGrid,
                 fog_circle:     &BitGrid,
                 fog_radius:     usize,
                 uni_width:      usize,
                 uni_height:     usize,
                 center_row_idx: usize,
                 center_col_idx: usize,
                 bits_to_clear:  u64) {

        if bits_to_clear == 0 {
            return; // nothing to do
        }
        debug!("---");

        // Iterate over every u64 in a rectangular region of `player_fog`, ANDing together the
        // shifted u64s of `fog_circle` according to `bits_to_clear`, so as to only perform a
        // single `&=` in `player_fog`.
        // EXPLANATION OF VAR NAMES: "_idx" indicates this is a word index; otherwise, it's a game
        // coord.

        // Get the highest and lowest bits in bits_to_clear
        let mut col_of_highest_to_clear = center_col_idx * 64;
        for shift in (0..64).rev() {
            if bits_to_clear & (1 << shift) > 0 {
                break;
            }
            col_of_highest_to_clear += 1;
        }
        let mut col_of_lowest_to_clear  = center_col_idx * 64 + 63;
        for shift in 0..64 {
            if bits_to_clear & (1 << shift) > 0 {
                break;
            }
            col_of_lowest_to_clear -= 1;
        }
        debug!("bits_to_clear: row {} and cols range [{}, {}]", center_row_idx, col_of_highest_to_clear, col_of_lowest_to_clear);

        // Get the bounds in terms of game coordinates (from col_left to col_right, inclusive,
        // and from row_top to row_bottom, inclusive).
        let row_top    = (uni_height + center_row_idx - (fog_radius - 1)) % uni_height;
        let row_bottom = (center_row_idx + fog_radius - 1) % uni_height;
        let col_left   = (uni_width + col_of_highest_to_clear - (fog_radius - 1)) % uni_width;
        let col_right  = (col_of_lowest_to_clear + fog_radius - 1) % uni_width;
        debug!("row_(top,bottom) range is [{}, {}]", row_top, row_bottom);
        debug!("col_(left,right) range is [{}, {}]", col_left, col_right);

        // Convert cols to col_idxes
        let col_idx_left  = col_left/64;
        let col_idx_right = col_right/64;

        let mut row_idx = row_top;
        let uni_word_width = uni_width/64;
        loop {
            //debug!("row_idx is {} (out of height {})", row_idx, uni_height);
            let mut col_idx = col_idx_left;
            loop {
                debug!("row {}, col range [{}, {}]", row_idx, col_idx*64, col_idx*64+63);
                //debug!("col_idx is {} (out of word_width {}); stopping after {}", col_idx, uni_word_width, col_idx_right);
                let mut mask = u64::max_value();
                for shift in (0..64).rev() {
                    if mask == 0 {
                        break;
                    }
                    if bits_to_clear & (1 << shift) > 0 {
                        let fog_row_idx = (uni_height  +  row_idx - center_row_idx + (fog_radius - 1)) % uni_height;
                        let current_highest_col = col_idx * 64;
                        let current_lowest_col  = col_idx * 64 + 63;
                        for fog_col_idx in 0 .. fog_circle.width_in_words() {
                            let fog_highest_col = (uni_width + center_col_idx*64 + (63 - shift) - (fog_radius - 1)) % uni_width;
                            let fog_lowest_col  = (uni_width + center_col_idx*64 + (63 - shift) - (fog_radius - 1) + 63) % uni_width;
                            debug!("  fog col range [{}, {}]", fog_highest_col, fog_lowest_col);

                            if current_highest_col == fog_highest_col && current_lowest_col == fog_lowest_col {
                                mask &= fog_circle[fog_row_idx][fog_col_idx];
                                debug!("  mask is now {:016x}, cleared by fog circle R{}, Ci{}, no shift", mask, fog_row_idx, fog_col_idx);
                            } else if current_highest_col <= fog_lowest_col && fog_lowest_col < current_lowest_col {
                                // we need to double negate so that shifting results in 1s, not 0s
                                mask &= !(!fog_circle[fog_row_idx][fog_col_idx] << (current_lowest_col - fog_lowest_col));
                                debug!("  fog word is {:016x}", fog_circle[fog_row_idx][fog_col_idx]);
                                debug!("  mask is now {:016x}, cleared by fog circle R{}, Ci{}, fog circle << {}", mask, fog_row_idx, fog_col_idx, current_lowest_col - fog_lowest_col);
                            } else if current_highest_col < fog_highest_col && fog_highest_col <= current_lowest_col {
                                mask &= !(!fog_circle[fog_row_idx][fog_col_idx] >> (fog_highest_col - current_highest_col));
                                debug!("  fog word is {:016x}", fog_circle[fog_row_idx][fog_col_idx]);
                                debug!("  mask is now {:016x}, cleared by fog circle R{}, Ci{}, fog circle >> {}", mask, fog_row_idx, fog_col_idx, fog_highest_col - current_highest_col);
                            }
                        }
                    }
                }
                player_fog[row_idx][col_idx] &= mask;

                if col_idx == col_idx_right {
                    break;
                }
                col_idx = (col_idx + 1) % uni_word_width;
            }

            if row_idx == row_bottom {
                break;
            }
            row_idx = (row_idx + 1) % uni_height;
        }
    }


    /// Iterate over every non-dead cell in the universe for the current generation. `region` is
    /// the rectangular area used for restricting results. `visibility` is an optional player_id;
    /// if specified, causes cells not visible to the player to be passed as `CellState::Fog` to the
    /// callback.
    /// 
    /// Callback receives (`col`, `row`, `cell_state`).
    /// 
    /// # Panics
    /// 
    /// Does numerous consistency checks on the bitmaps, and panics if inconsistencies are found.
    //XXX non_dead_cells_in_region
    pub fn each_non_dead(&self, region: Region, visibility: Option<usize>, callback: &mut FnMut(usize, usize, CellState)) {
        let cells = &self.gen_states[self.state_index].cells;
        let wall  = &self.gen_states[self.state_index].wall_cells;
        let known = &self.gen_states[self.state_index].known;
        let opt_player_state = if let Some(player_id) = visibility {
            Some(&self.gen_states[self.state_index].player_states[player_id])
        } else { None };
        let mut col;
        for row in 0 .. self.height {
            let cells_row = &cells[row];
            let wall_row  = &wall [row];
            let known_row = &known[row];
            if (row as isize) >= region.top() && (row as isize) < (region.top() + region.height() as isize) {
                col = 0;
                for col_idx in 0 .. self.width_in_words {
                    let cells_word = cells_row[col_idx];
                    let wall_word  = wall_row [col_idx];
                    let known_word = known_row[col_idx];
                    let opt_player_words;
                    if let Some(player_state) = opt_player_state {
                        let player_cells_word = player_state.cells[row][col_idx];
                        let player_fog_word   = player_state.fog[row][col_idx];
                        opt_player_words = Some((player_cells_word, player_fog_word));
                    } else {
                        opt_player_words = None;
                    }
                    for shift in (0..64).rev() {
                        if (col as isize) >= region.left() &&
                            (col as isize) < (region.left() + region.width() as isize) {
                            let mut state = CellState::Wall;
                            let c = (cells_word>>shift)&1 == 1;
                            let w = (wall_word >>shift)&1 == 1;
                            let k = (known_word>>shift)&1 == 1;
                            if c && w {
                                panic!("Cannot be both cell and wall at ({}, {})", col, row);
                            }
                            if !k && ((c && !w) || (!c && w)) {
                                panic!("Unspecified invalid state at ({}, {})", col, row);
                            }
                            if c && !w && k {
                                // It's known and it's a cell; check cells + fog for every player
                                // (expensive step since this is per-bit).

                                let mut opt_player_id = None;
                                for player_id in 0 .. self.num_players {
                                    let player_state = &self.gen_states[self.state_index].player_states[player_id];
                                    let pc = (player_state.cells[row][col_idx] >> shift) & 1 == 1;
                                    let pf = (player_state.fog[row][col_idx] >> shift) & 1 == 1;
                                    if pc && pf {
                                        panic!("Player cell and player fog at ({}, {}) for player {}", col, row, player_id);
                                    }
                                    if pc {
                                        if let Some(other_player_id) = opt_player_id {
                                            panic!("Cell ({}, {}) belongs to player {} and player {}!", col, row, other_player_id, player_id);
                                        }
                                        opt_player_id = Some(player_id);
                                    }
                                }
                                state = CellState::Alive(opt_player_id);
                            } else {
                                // (B) other states
                                if !c && !w {
                                    state = if k { CellState::Dead } else { CellState::Fog };
                                } else if !c && w {
                                    state = CellState::Wall;
                                }
                            }
                            if let Some((player_cells_word, player_fog_word)) = opt_player_words {
                                let pc = (player_cells_word>>shift)&1 == 1;
                                let pf = (player_fog_word>>shift)&1 == 1;
                                if !k && pc {
                                    panic!("Player can't have cells where unknown, at ({}, {})", col, row);
                                }
                                if w && pc {
                                    panic!("Player can't have cells where wall, at ({}, {})", col, row);
                                }
                                if pf {
                                    state = CellState::Fog;
                                }
                            }
                            if state != CellState::Dead {
                                callback(col, row, state);
                            }
                        }
                        col += 1;
                    }
                }
            }
        }
    }


    /// Iterate over every non-dead cell in the universe for the current generation.
    /// `visibility` is an optional player_id, allowing filtering based on fog.
    /// Callback receives (col, row, cell_state).
    pub fn each_non_dead_full(&self, visibility: Option<usize>, callback: &mut FnMut(usize, usize, CellState)) {
        self.each_non_dead(self.region(), visibility, callback);
    }


    /// Get a Region of the same size as the universe.
    pub fn region(&self) -> Region {
        Region::new(0, 0, self.width, self.height)
    }


    /// Copies from `src` BitGrid to this GenState as the player specified by `opt_player_id`,
    /// unless `opt_player_id` is `None`.
    ///
    /// This function is similar to `GenState::copy_from_bit_grid` except that 1) when a `player_id`
    /// is specified, the specified player's writable region is used, and 2) the latest generation
    /// is written to.
    ///
    /// Panics if `opt_player_id` is `Some(player_id)` and `player_id` is out of range.
    pub fn copy_from_bit_grid(&mut self, src: &BitGrid, dst_region: Region, opt_player_id: Option<usize>) {
        let region;
        if let Some(player_id) = opt_player_id {
            if let Some(_region) = dst_region.intersection(self.player_writable[player_id]) {
                region = _region;
            } else {
                // nothing to do because `dst_region` completely outside of player's writable region
                return;
            }
        } else {
            region = dst_region;
        }
        let latest_gen = &mut self.gen_states[self.state_index];
        latest_gen.copy_from_bit_grid(src, region, opt_player_id);
    }


    // return Ok(Some(new_gen)) or Ok(None) if nothing new
    // The "nothing new" case can happen if:
    //      - the generation to be applied is already present
    //      - there is already a greater generation present
    //      - the base generation of this diff could not be found
    // return Err("invalid - too large difference, gen0:<num> gen1:<num>") if the difference
    // between `diff.gen0` and `diff.gen1` is too large.
    // gen0 must be less than gen1, otherwise a panic results
    // Note: if pattern is invalid (that is, `to_grid` would return an error), the Universe will
    // not be restored to its original state.
    pub fn apply(&mut self, diff: &GenStateDiff, visibility: Option<usize>) -> Result<Option<usize>, String> {
        assert!(diff.gen0 < diff.gen1, format!("expected gen0 < gen1, but {} >= {}",
                                               diff.gen0, diff.gen1));
        // if diff too large, return Err(...)
        let gen_state_len = self.gen_states.len();
        if diff.gen1 - diff.gen0 >= gen_state_len {
            return Err(format!("diff is across too many generations to be applied: {} >= {}",
                               diff.gen1 - diff.gen0, gen_state_len));
        }

        // 1) find the gen0 in our gen_states; if not found, return Ok(None)
        let mut opt_gen0_idx = None;
        for i in 0..self.gen_states.len() {
            if let Some(gen) = self.gen_states[i].gen_or_none {
                if gen == diff.gen0 {
                    opt_gen0_idx = Some(i);
                }
                // 2) ensure that no generation is >= gen1; if so, return Ok(None)
                if gen >= diff.gen1 {
                    return Ok(None);
                }
            }
        }
        if opt_gen0_idx.is_none() {
            return Ok(None);
        }
        let gen0_idx = opt_gen0_idx.unwrap();

        // 3) make room for the new gen_state (make room in the circular buffer)
        for i in 0..self.gen_states.len() {
            if let Some(gen) = self.gen_states[i].gen_or_none {
                if gen <= diff.gen1 - gen_state_len {
                    self.gen_states[i].gen_or_none = None; // indicate uninitialized
                }
            }
        }

        // 4a) copy from the gen_state for gen0 to what will be the gen_state for gen1
        // TODO: This needs some serious cleanup
        let gen1_idx = (gen0_idx + diff.gen1 - diff.gen0) % gen_state_len;
        let cut_idx = cmp::max(gen0_idx, gen1_idx);
        {
            let (lower, upper): (&mut [GenState], &mut [GenState]) = self.gen_states.split_at_mut(cut_idx);
            if gen1_idx < cut_idx {
                lower[gen1_idx].clear();
                upper[gen0_idx - cut_idx].copy(&mut lower[gen1_idx]); // this is an |= operation, hence the clear before this
            } else {
                upper[gen1_idx - cut_idx].clear();
                lower[gen0_idx].copy(&mut upper[gen1_idx - cut_idx]); // this is an |= operation, hence the clear before this
            }
        }

        // 4b) apply the diff!
        diff.pattern.to_grid(&mut self.gen_states[gen1_idx], visibility)?;

        // 5) update self.generation, self.state_index, and self.gen_states[gen1_idx].gen_or_none
        let new_gen = diff.gen1;
        self.generation = new_gen;
        self.state_index = gen1_idx;
        self.gen_states[gen1_idx].gen_or_none = Some(new_gen);

        Ok(Some(new_gen))
    }
}


impl CharGrid for Universe {
    fn is_valid(ch: char) -> bool {
        GenState::is_valid(ch)
    }

    fn write_at_position(&mut self, _col: usize, _row: usize, _ch: char, _visibility: Option<usize>) {
        unimplemented!("This interface is not intended for modifying Universes");
    }

    /// Return width in cells.
    fn width(&self) -> usize {
        return self.width;
    }


    /// Return height in cells.
    fn height(&self) -> usize {
        return self.height;
    }


    fn get_run(&self, col: usize, row: usize, visibility: Option<usize>) -> (usize, char) {
        let gen_state = &self.gen_states[self.state_index];
        gen_state.get_run(col, row, visibility)
    }
}


/// Rectangular area within a `Universe`.
#[derive(Eq,PartialEq,Ord,PartialOrd,Copy,Clone,Debug)]
pub struct Region {
    left:   isize,
    top:    isize,
    width:  usize,
    height: usize,
}

/// A region is a rectangular area within a `Universe`. All coordinates are game coordinates.
impl Region {
    /// Creates a new region given x and y coordinates of top-left corner, and width and height,
    /// all in units of cells (game coordinates). Width and height must both be positive.
    pub fn new(left: isize, top: isize, width: usize, height: usize) -> Self {
        assert!(width != 0);
        assert!(height != 0);

        Region {
            left:   left,
            top:    top,
            width:  width,
            height: height,
        }
    }

    /// Returns the x coordinate of the leftmost cells of the Region, in game coordinates.
    pub fn left(&self) -> isize {
        self.left
    }

    /// Returns the x coordinate of the rightmost cells of the Region, in game coordinates.
    pub fn right(&self) -> isize {
        self.left + (self.width as isize) - 1
    }

    /// Returns the y coordinate of the uppermost cells of the Region, in game coordinates.
    pub fn top(&self) -> isize {
        self.top
    }

    /// Returns the y coordinate of the lowermost cells of the Region, in game coordinates.
    pub fn bottom(&self) -> isize {
        self.top + (self.height as isize) - 1
    }

    /// Returns the width of the Region (along x axis), in game coordinates. 
    pub fn width(&self) -> usize {
        self.width
    }

    /// Returns the height of the Region (along y axis), in game coordinates. 
    pub fn height(&self) -> usize {
        self.height
    }

    /// Determines whether the specified cell is part of the Region. 
    pub fn contains(&self, col: isize, row: isize) -> bool {
        self.left    <= col &&
        col <= self.right() &&
        self.top     <= row &&
        row <= self.bottom()
    }

    pub fn intersection(&self, other: Region) -> Option<Region> {
        let left = cmp::max(self.left(), other.left());
        let right = cmp::min(self.right(), other.right());
        if left > right {
            return None;
        }
        let width = right - left + 1;
        let top = cmp::max(self.top(), other.top());
        let bottom = cmp::min(self.bottom(), other.bottom());
        if top > bottom {
            return None;
        }
        let height = bottom - top + 1;
        Some(Region::new(left, top, width as usize, height as usize))
    }
}


#[cfg(test)]
mod universe_tests {
    use super::*;
    use rle::Pattern;

    fn generate_test_universe_with_default_params() -> Universe {
        let player0 = PlayerBuilder::new(Region::new(100, 70, 34, 16));   // used for the glider gun and predefined patterns
        let player1 = PlayerBuilder::new(Region::new(0, 0, 80, 80));
        let players = vec![player0, player1];

        let bigbang = BigBang::new()
            .width(256)
            .height(128)
            .server_mode(true)
            .history(16)
            .fog_radius(9)
            .add_players(players)
            .birth();

        bigbang.unwrap()
    }

    #[test]
    fn new_universe_with_valid_dims() {
        let uni = generate_test_universe_with_default_params();
        let universe_as_region = Region::new(0, 0, 256, 128);

        assert_eq!(uni.width(), 256);
        assert_eq!(uni.height(), 128);
        assert_eq!(uni.region(), universe_as_region);
    }

    #[test]
    fn new_universe_with_bad_dims() {

        let player0 = PlayerBuilder::new(Region::new(100, 70, 34, 16));   // used for the glider gun and predefined patterns
        let player1 = PlayerBuilder::new(Region::new(0, 0, 80, 80));
        let players = vec![player0, player1];

        let mut bigbang = BigBang::new()
            .width(256)
            .height(128)
            .server_mode(true)
            .history(16)
            .fog_radius(9)
            .add_players(players);

        bigbang = bigbang.width(255);

        let uni_result1 = bigbang.birth();
        assert!(uni_result1.is_err());

        bigbang = bigbang.width(256).height(0);
        let uni_result2 = bigbang.birth();
        assert!(uni_result2.is_err());

        bigbang = bigbang.width(0).height(256);
        let uni_result3 = bigbang.birth();
        assert!(uni_result3.is_err());
    }

    #[test]
    fn new_universe_first_gen_is_one() {
        let uni = generate_test_universe_with_default_params();
        assert_eq!(uni.latest_gen(), 1);
    }

    #[test]
    #[should_panic]
    fn universe_with_no_gens_panics() {
        let mut uni = generate_test_universe_with_default_params();
        uni.generation = 0;
        uni.latest_gen();
    }

    #[test]
    fn next_single_gen_test_data1_with_wrapping() {
        // glider, blinker, glider
        let nw = 0x0000000000000000;
        let n  = 0x0000000400000002;
        let ne = 0x8000000000000000;
        let w  = 0x0000000000000001;
        let cen= 0xC000000400000001;
        let e  = 0x8000000000000000;
        let sw = 0x0000000000000000;
        let s  = 0x8000000400000001;
        let se = 0x0000000000000000;
        let next_center = Universe::next_single_gen(nw, n, ne, w, cen, e, sw, s, se);
        assert_eq!(next_center, 0xC000000E00000002);
    }

    #[test]
    fn next_test_data1() {
        let mut uni = generate_test_universe_with_default_params();

        // r-pentomino
        let _ = uni.toggle(16, 15, 0);
        let _ = uni.toggle(17, 15, 0);
        let _ = uni.toggle(15, 16, 0);
        let _ = uni.toggle(16, 16, 0);
        let _ = uni.toggle(16, 17, 0);

        let gens = 20;
        for _ in 0..gens {
            uni.next();
        }
        assert_eq!(uni.latest_gen(), gens + 1);
    }

    #[test]
    fn set_unchecked_with_valid_rows_and_cols() {
        let mut uni = generate_test_universe_with_default_params();
        let max_width = uni.width()-1;
        let max_height = uni.height()-1;
        let mut cell_state;
        
        for x in 0.. max_width {
            for y in 0..max_height {
                cell_state = uni.get_cell_state(x,y, None);
                assert_eq!(cell_state, CellState::Dead);
            }
        }

        uni.set_unchecked(0, 0, CellState::Alive(None));
        cell_state = uni.get_cell_state(0,0, None);
        assert_eq!(cell_state, CellState::Alive(None));

        uni.set_unchecked(max_width, max_height, CellState::Alive(None));
        assert_eq!(cell_state, CellState::Alive(None));

        uni.set_unchecked(55, 55, CellState::Alive(None));
        assert_eq!(cell_state, CellState::Alive(None));
   }

    #[test]
    #[should_panic]
    fn set_unchecked_with_invalid_rols_and_cols_panics() {
        let mut uni = generate_test_universe_with_default_params();
        uni.set_unchecked(257, 129, CellState::Alive(None));
    }

    #[test]
    fn universe_cell_states_are_dead_on_creation() {
        let mut uni = generate_test_universe_with_default_params();
        let max_width = uni.width()-1;
        let max_height = uni.height()-1;
        
        for x in 0..max_width {
            for y in 0..max_height {
                let cell_state = uni.get_cell_state(x,y, None);
                assert_eq!(cell_state, CellState::Dead);
            }
        }
    }

    #[test]
    fn set_checked_verify_players_remain_within_writable_regions() {
        let mut uni = generate_test_universe_with_default_params();
        let max_width = uni.width()-1;
        let max_height = uni.height()-1;
        let player_id = 1; // writing into player 1's regions
        let alive_player_cell = CellState::Alive(Some(player_id));
        let mut cell_state;

        // Writable region OK, Transitions to Alive
        uni.set(0, 0, alive_player_cell, player_id);
        cell_state = uni.get_cell_state(0,0, Some(player_id));
        assert_eq!(cell_state, alive_player_cell);

        // This should be dead as it is outside the writable region
        uni.set(max_width, max_height, alive_player_cell, player_id);
        cell_state = uni.get_cell_state(max_width, max_height, Some(player_id));
        assert_eq!(cell_state, CellState::Dead);

        // Writable region OK, transitions to Alive
        uni.set(55, 55, alive_player_cell, player_id);
        cell_state = uni.get_cell_state(55, 55, Some(player_id));
        assert_eq!(cell_state, alive_player_cell);

        // Outside of player_id's writable region which will remain unchanged
        uni.set(81, 81, alive_player_cell, player_id);
        cell_state = uni.get_cell_state(81, 81, Some(player_id));
        assert_eq!(cell_state, CellState::Dead);
    }

    #[test]
    fn set_checked_cannot_set_a_fog_cell() {
        let mut uni = generate_test_universe_with_default_params();
        let player_id = 1; // writing into player 1's regions
        let alive_player_cell = CellState::Alive(Some(player_id));
        let state_index = uni.state_index;

        // Let's hardcode this and try to set a fog'd cell
        // within what was a players writable region.
        uni.gen_states[state_index].player_states[player_id].fog[0][0] |= 1<<63;

        uni.set(0, 0, alive_player_cell, player_id);
        let cell_state = uni.get_cell_state(0,0, Some(player_id));
        assert_eq!(cell_state, CellState::Dead);
    }


    #[test]
    fn toggle_unchecked_cell_toggled_is_owned_by_player() {
        let mut uni = generate_test_universe_with_default_params();
        let state_index = uni.state_index;
        let row = 0;
        let col = 0;
        let bit = 63;
        let player_one_opt = Some(0);
        let player_two_opt = Some(1);

        // Should transition from dead to alive. Player one will have their cell set, player two
        // will not
        assert_eq!(uni.toggle_unchecked(row, col, player_one_opt), CellState::Alive(player_one_opt));
        assert_eq!(uni.gen_states[state_index].player_states[player_one_opt.unwrap()].cells[row][col] >> bit, 1);
        assert_eq!(uni.gen_states[state_index].player_states[player_two_opt.unwrap()].cells[row][col] >> bit, 0);
    }

    #[test]
    fn toggle_unchecked_cell_toggled_by_both_players_repetitively() {
        let mut uni = generate_test_universe_with_default_params();
        let state_index = uni.state_index;
        let row = 0;
        let col = 0;
        let bit = 63;
        let player_one_opt = Some(0);
        let player_two_opt = Some(1);

        // Should transition from dead to alive. Player one will have their cell set, player two
        // will not
        assert_eq!(uni.toggle_unchecked(row, col, player_one_opt), CellState::Alive(player_one_opt));
        assert_eq!(uni.gen_states[state_index].player_states[player_one_opt.unwrap()].cells[row][col] >> bit, 1);
        assert_eq!(uni.gen_states[state_index].player_states[player_two_opt.unwrap()].cells[row][col] >> bit, 0);

        // Player two will now toggle the cell, killing it as it was previously alive.
        // Player one will be cleared as a result, the cell will not be set at all.
        // Notice we are not checking for writable regions here (unchecked doesn't care) so this
        // runs through
        assert_eq!(uni.toggle_unchecked(row, col, player_two_opt), CellState::Dead);
        assert_eq!(uni.gen_states[state_index].player_states[player_one_opt.unwrap()].cells[row][col] >> bit, 0);
        assert_eq!(uni.gen_states[state_index].player_states[player_two_opt.unwrap()].cells[row][col] >> bit, 0);
    }

    #[test]
    fn toggle_checked_outside_a_player_writable_region_fails() {
        let mut uni = generate_test_universe_with_default_params();
        let player_one = 0;
        let player_two = 1;
        let row = 0;
        let col = 0;

        assert_eq!(uni.toggle(row, col, player_one), Err(()));
        assert_eq!(uni.toggle(row, col, player_two).unwrap(), CellState::Alive(Some(player_two)));
    }

    #[test]
    fn toggle_checked_players_cannot_toggle_a_wall_cell() {
        let mut uni = generate_test_universe_with_default_params();
        let player_one = 0;
        let player_two = 1;
        let row = 0;
        let col = 0;
        let state_index = uni.state_index;

        uni.gen_states[state_index].wall_cells.modify_bits_in_word(row, col, 1<<63, BitOperation::Set);

        assert_eq!(uni.toggle(row, col, player_one), Err(()));
        assert_eq!(uni.toggle(row, col, player_two), Err(()));
    }

    #[test]
    fn toggle_checked_players_can_toggle_an_known_cell_if_writable() {
        let mut uni = generate_test_universe_with_default_params();
        let player_one = 0;
        let player_two = 1;
        let row = 0;
        let col = 0;
        let state_index = uni.state_index;

        uni.gen_states[state_index].known.modify_bits_in_word(row, col, 1<<63, BitOperation::Set);

        assert_eq!(uni.toggle(row, col, player_one), Err(()));
        assert_eq!(uni.toggle(row, col, player_two), Ok(CellState::Alive(Some(player_two))));
    }

    #[test]
    fn toggle_checked_players_cannot_toggle_an_unknown_cell() {
        let mut uni = generate_test_universe_with_default_params();
        let player_one = 0;
        let player_two = 1;
        let row = 0;
        let col = 0;
        let state_index = uni.state_index;

        uni.gen_states[state_index].known.modify_bits_in_word(row, col, 1<<63, BitOperation::Clear);

        assert_eq!(uni.toggle(row, col, player_one), Err(()));
        assert_eq!(uni.toggle(row, col, player_two), Err(()));
    }

    #[test]
    fn contagious_one_with_all_neighbors_set() {
        let north = u64::max_value();
        let northwest = u64::max_value();
        let northeast = u64::max_value();
        let west = u64::max_value();
        let mut center = u64::max_value();
        let east = u64::max_value();
        let southwest = u64::max_value();
        let south = u64::max_value();
        let southeast = u64::max_value();


        let mut output = Universe::contagious_one(northwest, north, northeast, west, center, east, southwest, south, southeast);
        assert_eq!(output, u64::max_value());

        center &= !(0x0000000F00000000);

        output = Universe::contagious_one(northwest, north, northeast, west, center, east, southwest, south, southeast);
        // 1 bit surrounding 'F', and inclusive, are cleared
        assert_eq!(output, 0xFFFFFFFFFFFFFFFF);
    }

    #[test]
    fn contagious_zero_with_all_neighbors_set() {
        let north = u64::max_value();
        let northwest = u64::max_value();
        let northeast = u64::max_value();
        let west = u64::max_value();
        let mut center = u64::max_value();
        let east = u64::max_value();
        let southwest = u64::max_value();
        let south = u64::max_value();
        let southeast = u64::max_value();


        let mut output = Universe::contagious_zero(northwest, north, northeast, west, center, east, southwest, south, southeast);
        assert_eq!(output, u64::max_value());

        center &= !(0x0000000F00000000);

        output = Universe::contagious_zero(northwest, north, northeast, west, center, east, southwest, south, southeast);
        // 1 bit surrounding 'F', and inclusive, are cleared
        assert_eq!(output, 0xFFFFFFE07FFFFFFF);
    }

    #[test]
    fn verify_fog_circle_bitmap_generation() {
        let mut uni = generate_test_universe_with_default_params();

        let fog_radius_of_nine = vec![
            vec![0xf007ffffffffffff],
            vec![0xe003ffffffffffff],
            vec![0xc001ffffffffffff],
            vec![0x8000ffffffffffff],
            vec![0x00007fffffffffff],
            vec![0x00007fffffffffff],
            vec![0x00007fffffffffff],
            vec![0x00007fffffffffff],
            vec![0x00007fffffffffff],
            vec![0x00007fffffffffff],
            vec![0x00007fffffffffff],
            vec![0x00007fffffffffff],
            vec![0x00007fffffffffff],
            vec![0x8000ffffffffffff],
            vec![0xc001ffffffffffff],
            vec![0xe003ffffffffffff],
            vec![0xf007ffffffffffff]];
        uni.fog_radius = 9;
        uni.generate_fog_circle_bitmap();
        assert_eq!(fog_radius_of_nine, uni.fog_circle.0);

        let fog_radius_of_four = vec![
            vec![0x83ffffffffffffff],
            vec![0x01ffffffffffffff],
            vec![0x01ffffffffffffff],
            vec![0x01ffffffffffffff],
            vec![0x01ffffffffffffff],
            vec![0x01ffffffffffffff],
            vec![0x83ffffffffffffff],
        ];
        uni.fog_radius = 4;
        uni.generate_fog_circle_bitmap();
        assert_eq!(fog_radius_of_four, uni.fog_circle.0);

        let fog_radius_of_thirtyfive = vec![
            vec![0xffffffc0001fffff, 0xffffffffffffffff, ],
            vec![0xfffffe000003ffff, 0xffffffffffffffff, ],
            vec![0xfffff00000007fff, 0xffffffffffffffff, ],
            vec![0xffffc00000001fff, 0xffffffffffffffff, ],
            vec![0xffff0000000007ff, 0xffffffffffffffff, ],
            vec![0xfffe0000000003ff, 0xffffffffffffffff, ],
            vec![0xfffc0000000001ff, 0xffffffffffffffff, ],
            vec![0xfff000000000007f, 0xffffffffffffffff, ],
            vec![0xffe000000000003f, 0xffffffffffffffff, ],
            vec![0xffc000000000001f, 0xffffffffffffffff, ],
            vec![0xff8000000000000f, 0xffffffffffffffff, ],
            vec![0xff00000000000007, 0xffffffffffffffff, ],
            vec![0xfe00000000000003, 0xffffffffffffffff, ],
            vec![0xfe00000000000003, 0xffffffffffffffff, ],
            vec![0xfc00000000000001, 0xffffffffffffffff, ],
            vec![0xf800000000000000, 0xffffffffffffffff, ],
            vec![0xf000000000000000, 0x7fffffffffffffff, ],
            vec![0xf000000000000000, 0x7fffffffffffffff, ],
            vec![0xe000000000000000, 0x3fffffffffffffff, ],
            vec![0xe000000000000000, 0x3fffffffffffffff, ],
            vec![0xc000000000000000, 0x1fffffffffffffff, ],
            vec![0xc000000000000000, 0x1fffffffffffffff, ],
            vec![0xc000000000000000, 0x1fffffffffffffff, ],
            vec![0x8000000000000000, 0x0fffffffffffffff, ],
            vec![0x8000000000000000, 0x0fffffffffffffff, ],
            vec![0x8000000000000000, 0x0fffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x0000000000000000, 0x07ffffffffffffff, ],
            vec![0x8000000000000000, 0x0fffffffffffffff, ],
            vec![0x8000000000000000, 0x0fffffffffffffff, ],
            vec![0x8000000000000000, 0x0fffffffffffffff, ],
            vec![0xc000000000000000, 0x1fffffffffffffff, ],
            vec![0xc000000000000000, 0x1fffffffffffffff, ],
            vec![0xc000000000000000, 0x1fffffffffffffff, ],
            vec![0xe000000000000000, 0x3fffffffffffffff, ],
            vec![0xe000000000000000, 0x3fffffffffffffff, ],
            vec![0xf000000000000000, 0x7fffffffffffffff, ],
            vec![0xf000000000000000, 0x7fffffffffffffff, ],
            vec![0xf800000000000000, 0xffffffffffffffff, ],
            vec![0xfc00000000000001, 0xffffffffffffffff, ],
            vec![0xfe00000000000003, 0xffffffffffffffff, ],
            vec![0xfe00000000000003, 0xffffffffffffffff, ],
            vec![0xff00000000000007, 0xffffffffffffffff, ],
            vec![0xff8000000000000f, 0xffffffffffffffff, ],
            vec![0xffc000000000001f, 0xffffffffffffffff, ],
            vec![0xffe000000000003f, 0xffffffffffffffff, ],
            vec![0xfff000000000007f, 0xffffffffffffffff, ],
            vec![0xfffc0000000001ff, 0xffffffffffffffff, ],
            vec![0xfffe0000000003ff, 0xffffffffffffffff, ],
            vec![0xffff0000000007ff, 0xffffffffffffffff, ],
            vec![0xffffc00000001fff, 0xffffffffffffffff, ],
            vec![0xfffff00000007fff, 0xffffffffffffffff, ],
            vec![0xfffffe000003ffff, 0xffffffffffffffff, ],
            vec![0xffffffc0001fffff, 0xffffffffffffffff, ],
            ];

        uni.fog_radius = 35;
        uni.generate_fog_circle_bitmap();
        assert_eq!(fog_radius_of_thirtyfive, uni.fog_circle.0);
    }

    #[test]
    fn generate_fog_circle_bitmap_fails_with_radius_zero() {
        let player0 = PlayerBuilder::new(Region::new(100, 70, 34, 16));   // used for the glider gun and predefined patterns
        let player1 = PlayerBuilder::new(Region::new(0, 0, 80, 80));
        let players = vec![player0, player1];

        let uni = BigBang::new()
            .width(256)
            .height(128)
            .server_mode(true)
            .history(16)
            .fog_radius(0)
            .add_players(players)
            .birth();

        assert!(uni.is_err());
    }

    #[test]
    fn clear_fog_with_standard_radius() {
        let player0 = PlayerBuilder::new(Region::new(100, 70, 34, 16));   // used for the glider gun and predefined patterns
        let player1 = PlayerBuilder::new(Region::new(0, 0, 80, 80));
        let players = vec![player0, player1];

        let mut uni = BigBang::new()
            .width(256)
            .height(128)
            .server_mode(true)
            .history(16)
            .fog_radius(4)
            .add_players(players)
            .birth()
            .unwrap();

        let history = uni.gen_states.len();
        let next_state_index = (uni.state_index + 1) % history;
        let player_id = 0;

        let gen_state_next = if uni.state_index < next_state_index {
            let (_, p1) = uni.gen_states.split_at_mut(next_state_index);
            &mut p1[player_id]
        } else {
            let (p0, _) = uni.gen_states.split_at_mut(next_state_index + 1);
            &mut p0[player_id]
        };
        let row_index_outside_of_p0_region = 1;
        let col_index_outside_of_p0_region = 1;
        let one_bit_to_clear = 1;

        Universe::clear_fog(&mut gen_state_next.player_states[player_id].fog, 
                            &uni.fog_circle, 
                            uni.fog_radius, 
                            uni.width, 
                            uni.height, 
                            row_index_outside_of_p0_region, 
                            col_index_outside_of_p0_region, 
                            one_bit_to_clear);

        for x in 0..4 {
            for y in 1..2 {
                assert_eq!(gen_state_next.player_states[0].fog[x][y], 0b1111111111111111111111111111111111111111111111111111111111110000);
            }
        }

    }

    #[test]
    fn each_non_dead_detects_some_cells() {
        let mut uni = generate_test_universe_with_default_params();
        let player1 = 1;

        // glider
        uni.toggle(16, 15, player1).unwrap();
        uni.toggle(17, 16, player1).unwrap();
        uni.toggle(15, 17, player1).unwrap();
        uni.toggle(16, 17, player1).unwrap();
        uni.toggle(17, 17, player1).unwrap();

        // just a wall, for no reason at all
        for col in 10..15 {
            uni.set_unchecked(col, 12, CellState::Wall);
        }

        let gens = 21;
        for _ in 0..gens {
            uni.next();
        }
        let mut cells_with_pos: Vec<(usize, usize, CellState)> = vec![];
        uni.each_non_dead(Region::new(0, 0, 80, 80), Some(player1), &mut |col, row, state| {
            cells_with_pos.push((col, row, state));
        });
        assert_eq!(cells_with_pos.len(), 10);
        assert_eq!(cells_with_pos, vec![(10, 12, CellState::Wall),
                                        (11, 12, CellState::Wall),
                                        (12, 12, CellState::Wall),
                                        (13, 12, CellState::Wall),
                                        (14, 12, CellState::Wall),
                                        (20, 21, CellState::Alive(Some(1))),
                                        (22, 21, CellState::Alive(Some(1))),
                                        (21, 22, CellState::Alive(Some(1))),
                                        (22, 22, CellState::Alive(Some(1))),
                                        (21, 23, CellState::Alive(Some(1)))]);

    }

    #[test]
    fn each_non_dead_detects_fog() {
        let mut uni = generate_test_universe_with_default_params();
        let player0 = 0;
        let player1 = 1;

        // blinker as player 1
        uni.toggle(16, 15, player1).unwrap();
        uni.toggle(16, 16, player1).unwrap();
        uni.toggle(16, 17, player1).unwrap();

        // attempt to view as different player
        uni.each_non_dead(Region::new(0, 0, 80, 80), Some(player0), &mut |col, row, state| {
            assert_eq!(state, CellState::Fog, "expected fog at col {} row {} but found {:?}", col, row, state);
        });
    }

    #[test]
    fn universe_copy_from_bit_grid_as_player() {
        let mut uni = generate_test_universe_with_default_params();
        let grid = Pattern("64o$64o!".to_owned()).to_new_bit_grid(64, 2).unwrap();

        let write_pattern_as = Some(1); // player 1
        let dst_region = Region::new(0, 0, 32, 3);

        uni.copy_from_bit_grid(&grid, dst_region, write_pattern_as);

        {
            let genstate = &uni.gen_states[uni.state_index];
            assert_eq!(genstate.cells.to_pattern(None).0, "32o$32o!".to_owned());
            assert_eq!(genstate.wall_cells.to_pattern(None).0, "!".to_owned());
            assert_eq!(genstate.player_states[0].cells.to_pattern(None).0, "!".to_owned());
            assert_eq!(genstate.player_states[0].fog[0][0], u64::max_value());  // complete fog for player 0
            assert_eq!(genstate.player_states[0].fog[1][0], u64::max_value());  // complete fog for player 0
            assert_eq!(genstate.player_states[1].cells.to_pattern(None).0, "32o$32o!".to_owned());
        }
    }

    #[test]
    fn universe_copy_from_bit_grid_as_player_out_of_range() {
        let mut uni = generate_test_universe_with_default_params();
        let grid = Pattern("64o$64o!".to_owned()).to_new_bit_grid(64, 2).unwrap();

        let write_pattern_as = Some(0); // player 0
        let dst_region = Region::new(0, 0, 32, 3); // out of range for player 0

        uni.copy_from_bit_grid(&grid, dst_region, write_pattern_as);

        {
            let genstate = &uni.gen_states[uni.state_index];
            assert_eq!(genstate.cells.to_pattern(None).0, "!".to_owned());
            assert_eq!(genstate.wall_cells.to_pattern(None).0, "!".to_owned());
            assert_eq!(genstate.player_states[0].cells.to_pattern(None).0, "!".to_owned()); // no player 0 cells
            assert_eq!(genstate.player_states[0].fog[0][0], u64::max_value());  // complete fog for player 0
            assert_eq!(genstate.player_states[0].fog[1][0], u64::max_value());  // complete fog for player 0
            assert_eq!(genstate.player_states[1].cells.to_pattern(None).0, "!".to_owned()); // no player 1 cells
        }
    }
}

#[cfg(test)]
mod genstate_tests {
    use super::*;
    use rle::Pattern;

    // Utilities
    fn make_gen_state() -> GenState {
        let player0 = PlayerBuilder::new(Region::new(100, 70, 34, 16));
        let player1 = PlayerBuilder::new(Region::new(0, 0, 80, 80));
        let players = vec![player0, player1];

        let mut uni = BigBang::new()
            .history(1)
            .add_players(players)
            .birth()
            .unwrap();
        uni.gen_states.pop().unwrap()
    }

    #[test]
    fn write_at_position_server_wall() {
        let mut genstate = make_gen_state();

        genstate.write_at_position(63, 0, 'W', None);
        assert_eq!(genstate.cells[0][0], 0);
        assert_eq!(genstate.wall_cells[0][0], 1);
        assert_eq!(genstate.known[0][0], u64::max_value());
        assert_eq!(genstate.player_states[0].cells[0][0], 0);
        assert_eq!(genstate.player_states[1].cells[0][0], 0);
    }

    #[test]
    fn write_at_position_server_wall_overwrite() {
        let mut genstate = make_gen_state();

        genstate.write_at_position(63, 0, 'b', None);
        genstate.write_at_position(63, 0, 'W', None);
        assert_eq!(genstate.cells[0][0], 0);
        assert_eq!(genstate.wall_cells[0][0], 1);
        assert_eq!(genstate.known[0][0], u64::max_value());
        assert_eq!(genstate.player_states[0].cells[0][0], 0);
        assert_eq!(genstate.player_states[1].cells[0][0], 0);
    }

    #[test]
    fn write_at_position_server_player0() {
        let mut genstate = make_gen_state();

        genstate.write_at_position(63, 0, 'A', None);
        assert_eq!(genstate.cells[0][0], 1);
        assert_eq!(genstate.wall_cells[0][0], 0);
        assert_eq!(genstate.known[0][0], u64::max_value());
        assert_eq!(genstate.player_states[0].cells[0][0], 1);
        assert_eq!(genstate.player_states[1].cells[0][0], 0);
    }

    #[test]
    fn write_at_position_server_player1() {
        let mut genstate = make_gen_state();

        genstate.write_at_position(63, 0, 'B', None);
        assert_eq!(genstate.cells[0][0], 1);
        assert_eq!(genstate.wall_cells[0][0], 0);
        assert_eq!(genstate.known[0][0], u64::max_value());
        assert_eq!(genstate.player_states[0].cells[0][0], 0);
        assert_eq!(genstate.player_states[1].cells[0][0], 1);
    }

    #[test]
    fn write_at_position_server_player0_then_1() {
        let mut genstate = make_gen_state();

        genstate.write_at_position(63, 0, 'A', None);
        genstate.write_at_position(63, 0, 'B', None);
        assert_eq!(genstate.cells[0][0], 1);
        assert_eq!(genstate.player_states[0].cells[0][0], 0);
        assert_eq!(genstate.player_states[1].cells[0][0], 1);
    }

    #[test]
    fn write_at_position_server_player1_then_unowned() {
        let mut genstate = make_gen_state();

        genstate.write_at_position(63, 0, 'B', None);
        genstate.write_at_position(63, 0, 'o', None);
        assert_eq!(genstate.cells[0][0], 1);
        assert_eq!(genstate.wall_cells[0][0], 0);
        assert_eq!(genstate.known[0][0], u64::max_value());
        assert_eq!(genstate.player_states[0].cells[0][0], 0);
        assert_eq!(genstate.player_states[1].cells[0][0], 0);
    }

    #[test]
    fn write_at_position_server_player1_then_blank() {
        let mut genstate = make_gen_state();

        genstate.write_at_position(63, 0, 'B', None);
        genstate.write_at_position(63, 0, 'b', None);
        assert_eq!(genstate.cells[0][0], 0);
        assert_eq!(genstate.wall_cells[0][0], 0);
        assert_eq!(genstate.known[0][0], u64::max_value());
        assert_eq!(genstate.player_states[0].cells[0][0], 0);
        assert_eq!(genstate.player_states[1].cells[0][0], 0);
    }

    #[test]
    fn write_at_position_as_player0_player1_then_blank() {
        let mut genstate = make_gen_state();

        genstate.write_at_position(63, 0, 'B', Some(0));
        genstate.write_at_position(63, 0, 'b', Some(0));
        assert_eq!(genstate.cells[0][0], 0);
        assert_eq!(genstate.wall_cells[0][0], 0);
        assert_eq!(genstate.known[0][0], u64::max_value());
        assert_eq!(genstate.player_states[0].cells[0][0], 0);
        assert_eq!(genstate.player_states[1].cells[0][0], 0);
    }

    #[test]
    fn write_at_position_as_player0_clears_only_that_players_fog() {
        let mut genstate = make_gen_state();

        // fog bits initially clear for both players
        genstate.player_states[0].fog[0][0] = 0;
        genstate.player_states[1].fog[0][0] = 0;
        genstate.write_at_position(63, 0, '?', Some(0));
        assert_eq!(genstate.cells[0][0], 0);
        assert_eq!(genstate.wall_cells[0][0], 0);
        assert_eq!(genstate.player_states[0].cells[0][0], 0);
        assert_eq!(genstate.player_states[0].fog[0][0], 1);
        assert_eq!(genstate.player_states[1].cells[0][0], 0);
        assert_eq!(genstate.player_states[1].fog[0][0], 0);
    }

    #[test]
    fn write_at_position_as_player0_sets_only_that_players_fog() {
        let mut genstate = make_gen_state();

        // fog bits initially set for both players
        genstate.player_states[0].fog[0][0] = 1;
        genstate.player_states[1].fog[0][0] = 1;
        genstate.write_at_position(63, 0, 'o', Some(0));
        assert_eq!(genstate.cells[0][0], 1);
        assert_eq!(genstate.wall_cells[0][0], 0);
        assert_eq!(genstate.player_states[0].cells[0][0], 0);
        assert_eq!(genstate.player_states[0].fog[0][0], 0);
        assert_eq!(genstate.player_states[1].cells[0][0], 0);
        assert_eq!(genstate.player_states[1].fog[0][0], 1);
    }

    #[test]
    fn gen_state_get_run_simple() {
        let mut genstate = make_gen_state();

        Pattern("o!".to_owned()).to_grid(&mut genstate, None).unwrap();
        assert_eq!(genstate.get_run(0, 0, None), (1, 'o'));
    }

    #[test]
    fn gen_state_get_run_wall() {
        let mut genstate = make_gen_state();

        Pattern("4W!".to_owned()).to_grid(&mut genstate, None).unwrap();
        assert_eq!(genstate.get_run(0, 0, None), (4, 'W'));
    }

    #[test]
    fn gen_state_get_run_wall_blank_in_front() {
        let mut genstate = make_gen_state();

        Pattern("12b4W!".to_owned()).to_grid(&mut genstate, None).unwrap();
        assert_eq!(genstate.get_run(12, 0, None), (4, 'W'));
    }

    #[test]
    fn gen_state_get_run_alternating_owned_unowned() {
        let mut genstate = make_gen_state();

        Pattern("15o3A2B9o!".to_owned()).to_grid(&mut genstate, None).unwrap();
        assert_eq!(genstate.get_run(0,      0, None), (15, 'o'));
        assert_eq!(genstate.get_run(15,     0, None), (3,  'A'));
        assert_eq!(genstate.get_run(15+3,   0, None), (2,  'B'));
        assert_eq!(genstate.get_run(15+3+2, 0, None), (9,  'o'));
    }

    #[test]
    fn gen_state_get_run_player0_fog() {
        let mut genstate = make_gen_state();

        // The following two are not really tests -- they are just consequences of the writable
        // areas set up for these players when creating the Universe, and are here for
        // "documentation".
        assert_eq!(genstate.player_states[0].fog[0][0], u64::max_value());  // complete fog for player 0
        assert_eq!(genstate.player_states[1].fog[0][0], 0);                 // no fog here for player 1

        let write_pattern_as = Some(1);   // avoid clearing fog for players other than player 1
        Pattern("o!".to_owned()).to_grid(&mut genstate, write_pattern_as).unwrap();
        let visibility = Some(0); // as player 0
        assert_eq!(genstate.get_run(0, 0, visibility), (genstate.width(), '?'));
    }

    #[test]
    fn gen_state_get_run_player1_no_fog() {
        let mut genstate = make_gen_state();

        let write_pattern_as = Some(1);   // avoid clearing fog for players other than player 1
        Pattern("o!".to_owned()).to_grid(&mut genstate, write_pattern_as).unwrap();
        let visibility = Some(1); // as player 1
        assert_eq!(genstate.get_run(0, 0, visibility), (1, 'o'));
    }

    #[test]
    fn gen_state_copy_from_bit_grid_simple() {
        let mut genstate = make_gen_state();
        let grid = Pattern("64o$64o!".to_owned()).to_new_bit_grid(64, 2).unwrap();

        let write_pattern_as = None;
        let dst_region = Region::new(0, 0, 32, 3);
        genstate.copy_from_bit_grid(&grid, dst_region, write_pattern_as);

        assert_eq!(genstate.cells.to_pattern(None).0, "32o$32o!".to_owned());
        assert_eq!(genstate.wall_cells.to_pattern(None).0, "!".to_owned());
        for player_id in 0..genstate.player_states.len() {
            assert_eq!(genstate.player_states[player_id].cells.to_pattern(None).0, "!".to_owned());
        }
    }

    #[test]
    fn gen_state_copy_from_bit_grid_as_player() {
        let mut genstate = make_gen_state();
        let grid = Pattern("64o$64o!".to_owned()).to_new_bit_grid(64, 2).unwrap();

        assert_eq!(genstate.player_states[0].fog[0][0], u64::max_value());  // complete fog for player 0
        assert_eq!(genstate.player_states[1].fog[0][0], 0);                 // no fog here for player 1

        let write_pattern_as = Some(1); // player 1
        let raw_dst_region = Region::new(0, 0, 32, 3);

        // intersect with player 1 writable region from make_gen_state()
        let dst_region = raw_dst_region.intersection(Region::new(0, 0, 80, 80)).unwrap();

        genstate.copy_from_bit_grid(&grid, dst_region, write_pattern_as);

        assert_eq!(genstate.cells.to_pattern(None).0, "32o$32o!".to_owned());
        assert_eq!(genstate.wall_cells.to_pattern(None).0, "!".to_owned());
        assert_eq!(genstate.player_states[0].cells.to_pattern(None).0, "!".to_owned());
        assert_eq!(genstate.player_states[0].fog[0][0], u64::max_value());  // complete fog for player 0
        assert_eq!(genstate.player_states[0].fog[1][0], u64::max_value());  // complete fog for player 0
        assert_eq!(genstate.player_states[1].cells.to_pattern(None).0, "32o$32o!".to_owned());
    }


    #[test]
    fn gen_state_pair_get_run_simple() {
        let gs0 = make_gen_state();
        let mut gs1 = make_gen_state();
        Pattern("o!".to_owned()).to_grid(&mut gs1, None).unwrap();

        let pair = GenStatePair {
            gen_state0: &gs0,
            gen_state1: &gs1,
        };

        assert_eq!(pair.get_run(0, 0, None), (1, 'o'));
        assert_eq!(pair.get_run(1, 0, None), (gs0.width() - 1, '"'));
    }

    #[test]
    fn gen_state_pair_get_run_simple2() {
        let mut gs0 = make_gen_state();
        Pattern("2o3b5o!".to_owned()).to_grid(&mut gs0, None).unwrap();
        let mut gs1 = make_gen_state();
        Pattern("2o3b5o!".to_owned()).to_grid(&mut gs1, None).unwrap();

        let pair = GenStatePair {
            gen_state0: &gs0,
            gen_state1: &gs1,
        };

        assert_eq!(pair.get_run(0, 0, None), (gs0.width(), '"'));
    }

    #[test]
    fn gen_state_pair_get_run_simple3() {
        let mut gs0 = make_gen_state();
        Pattern("4b5o!".to_owned()).to_grid(&mut gs0, None).unwrap();
        let mut gs1 = make_gen_state();
        Pattern("3b5o!".to_owned()).to_grid(&mut gs1, None).unwrap();

        let pair = GenStatePair {
            gen_state0: &gs0,
            gen_state1: &gs1,
        };

        assert_eq!(pair.get_run(0,       0, None), (3, '"'));
        assert_eq!(pair.get_run(3,       0, None), (1, 'o'));
        assert_eq!(pair.get_run(3+1,     0, None), (4, '"'));
        assert_eq!(pair.get_run(3+1+4,   0, None), (1, 'b'));
        assert_eq!(pair.get_run(3+1+4+1, 0, None), (gs0.width() - (3+1+4+1), '"'));
    }

    #[test]
    fn gen_state_diff_simple1() {
        let gs0 = make_gen_state();
        let mut gs1 = make_gen_state();
        Pattern("o!".to_owned()).to_grid(&mut gs1, None).unwrap();

        let gsdiff = gs0.diff(&gs1, None);
        assert_eq!(gsdiff.pattern.0.len(), 659);
        let mut gsdiff_pattern_iter = gsdiff.pattern.0.split('$');
        assert_eq!(gsdiff_pattern_iter.next().unwrap(), "o255\"");
        assert_eq!(gsdiff_pattern_iter.next().unwrap(), "256\"");
        assert_eq!(gsdiff_pattern_iter.next().unwrap(), "256\"");
        // if you keep doing this, you'll eventually get a string containing \r\n
    }

    #[test]
    fn gen_state_diff_and_restore_simple1() {
        let gs0 = make_gen_state();
        let mut gs1 = make_gen_state();
        let visibility = None;
        Pattern("o!".to_owned()).to_grid(&mut gs1, visibility).unwrap();

        let gsdiff = gs0.diff(&gs1, visibility);

        let mut new_gs = make_gen_state();

        gsdiff.pattern.to_grid(&mut new_gs, visibility).unwrap();
        assert_eq!(new_gs, gs1);
    }

    #[test]
    fn gen_state_diff_and_restore_complex1() {
        let gs0 = make_gen_state();
        let mut gs1 = make_gen_state();
        let visibility = None;
        let pat_str = "b2o23b2o21b$b2o23bo22b$24bobo22b$15b2o7b2o23b$2o13bobo31b$2o13bob2o30b$16b2o31b$16bo32b$44b2o3b$16bo27b2o3b$16b2o31b$2o13bob2o13bo3bo12b$2o13bobo13bo5bo7b2o2b$15b2o14bo13b2o2b$31b2o3bo12b$b2o30b3o13b$b2o46b$33b3o13b$31b2o3bo12b$31bo13b2o2b$31bo5bo7b2o2b$32bo3bo12b2$44b2o3b$44b2o3b5$37b2o10b$37bobo7b2o$39bo7b2o$37b3o9b$22bobo24b$21b3o25b$21b3o25b$21bo15b3o9b$25bobo11bo9b$21b2o4bo9bobo9b$16b2o4bo3b2o9b2o10b$15bobo6bo24b$15bo33b$14b2o!".to_owned();
        Pattern(pat_str).to_grid(&mut gs1, visibility).unwrap();

        let gsdiff = gs0.diff(&gs1, visibility);

        let mut new_gs = make_gen_state();

        gsdiff.pattern.to_grid(&mut new_gs, visibility).unwrap();
        assert_eq!(new_gs.gen_or_none, gs1.gen_or_none);
        assert_eq!(new_gs.cells, gs1.cells);
        assert_eq!(new_gs.known, gs1.known);
        assert_eq!(new_gs.wall_cells, gs1.wall_cells);
        for i in 0..new_gs.player_states.len() {
            assert_eq!(new_gs.player_states[i].cells, gs1.player_states[i].cells);
            //assert_eq!(new_gs.player_states[i].fog, gs1.player_states[i].fog);  // OK for fog to differ, because normally the fog would already
                                                                                  // be cleared where pattern is being written.
        }
    }

    #[test]
    fn gen_state_diff_and_restore_complex1_with_visibility() {
        let gs0 = make_gen_state();
        let mut gs1 = make_gen_state();
        let visibility = Some(1);
        let pat_str = "b2o23b2o21b$b2o23bo22b$24bobo22b$15b2o7b2o23b$2o13bobo31b$2o13bob2o30b$16b2o31b$16bo32b$44b2o3b$16bo27b2o3b$16b2o31b$2o13bob2o13bo3bo12b$2o13bobo13bo5bo7b2o2b$15b2o14bo13b2o2b$31b2o3bo12b$b2o30b3o13b$b2o46b$33b3o13b$31b2o3bo12b$31bo13b2o2b$31bo5bo7b2o2b$32bo3bo12b2$44b2o3b$44b2o3b5$37b2o10b$37bobo7b2o$39bo7b2o$37b3o9b$22bobo24b$21b3o25b$21b3o25b$21bo15b3o9b$25bobo11bo9b$21b2o4bo9bobo9b$16b2o4bo3b2o9b2o10b$15bobo6bo24b$15bo33b$14b2o!".to_owned();
        Pattern(pat_str).to_grid(&mut gs1, visibility).unwrap();

        let gsdiff = gs0.diff(&gs1, visibility);

        let mut new_gs = make_gen_state();

        gsdiff.pattern.to_grid(&mut new_gs, visibility).unwrap();
        assert_eq!(new_gs.gen_or_none, gs1.gen_or_none);
        assert_eq!(new_gs.cells, gs1.cells);
        assert_eq!(new_gs.known, gs1.known);
        assert_eq!(new_gs.wall_cells, gs1.wall_cells);
        for i in 0..new_gs.player_states.len() {
            assert_eq!(new_gs.player_states[i].cells, gs1.player_states[i].cells);
            assert_eq!(new_gs.player_states[i].fog, gs1.player_states[i].fog);
        }
    }
}


#[cfg(test)]
mod region_tests {
    use super::*;

    #[test]
    fn region_with_valid_dims() {
        let region = Region::new(1, 10, 100, 200);

        assert_eq!(region.left(), 1);
        assert_eq!(region.top(), 10);
        assert_eq!(region.height(), 200);
        assert_eq!(region.width(), 100);
        assert_eq!(region.right(), 100);
        assert_eq!(region.bottom(), 209);
    }
    
    #[test]
    fn region_with_valid_dims_negative_top_and_left() {
        let region = Region::new(-1, -10, 100, 200);

        assert_eq!(region.left(), -1);
        assert_eq!(region.top(), -10);
        assert_eq!(region.height(), 200);
        assert_eq!(region.width(), 100);
        assert_eq!(region.right(), 98);
        assert_eq!(region.bottom(), 189);
    }

    #[test]
    #[should_panic]
    fn region_with_bad_dims_panics() {
        Region::new(0, 0, 0, 0);
    }

    #[test]
    fn region_contains_a_valid_sub_region() {
        let region1 = Region::new(1, 10, 100, 200);
        let region2 = Region::new(-100, -200, 100, 200);

        assert!(region1.contains(50, 50));
        assert!(region2.contains(-50, -50));
    }
    
    #[test]
    fn region_does_not_contain_sub_region() {
        let region1 = Region::new(1, 10, 100, 200);
        let region2 = Region::new(-100, -200, 100, 200);

        assert!(!region1.contains(-50, -50));
        assert!(!region2.contains(50, 50));
    }

    #[test]
    fn region_no_intersection() {
        let region1 = Region::new(1, 10, 100, 200);
        let region2 = Region::new(-100, -200, 100, 200);
        assert_eq!(region1.intersection(region2), None);
        assert_eq!(region2.intersection(region1), None);
    }

    #[test]
    fn region_intersection_with_self() {
        let region1 = Region::new(1, 10, 100, 200);
        assert_eq!(region1.intersection(region1), Some(region1));
    }

    #[test]
    fn region_intersection_overlap() {
        let region1 = Region::new( 1,  10, 100, 200);
        let region2 = Region::new(90, 120, 100, 200);
        assert_eq!(region1.intersection(region2), Some(Region::new(90, 120, 11, 90)));
    }

    #[test]
    fn region_no_intersection_overlap_one_dim() {
        let region1 = Region::new(0, 0, 2, 2);
        let region2 = Region::new(3, 0, 2, 2);
        assert_eq!(region1.intersection(region2), None);
    }
}

#[cfg(test)]
mod cellstate_tests {
    use super::*;

    #[test]
    fn cell_states_as_char() {
        let dead = CellState::Dead;
        let alive = CellState::Alive(None);
        let player1 = CellState::Alive(Some(1));
        let player2 = CellState::Alive(Some(2));
        let wall = CellState::Wall;
        let fog = CellState::Fog;

        assert_eq!(dead.to_char(), 'b');
        assert_eq!(alive.to_char(), 'o');
        assert_eq!(player1.to_char(), 'B');
        assert_eq!(player2.to_char(), 'C');
        assert_eq!(wall.to_char(), 'W');
        assert_eq!(fog.to_char(), '?');
    }
}
