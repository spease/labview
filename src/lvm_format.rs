use super::errors::*;
use lvm;
use num;
use serde;
use std;

use serde::de::IntoDeserializer;

#[derive(Debug)]
struct Deserializer<R: std::io::BufRead> {
  line_current: String,
  line_index: usize,
  input: std::io::Lines<R>,
  separator: lvm::Separator,
  sequence_style: SequenceStyle,
}

impl<R: std::io::BufRead> Deserializer<R> {
  const BOOL_YES: &'static str = "Yes";
  const BOOL_NO: &'static str = "No";
  const BOOL_OPTIONS: &'static [&'static str] = &[Self::BOOL_NO, Self::BOOL_YES];
  const HEADER: &'static str = "LabVIEW Measurement";
  const HEADER_OPTIONS :&'static [&'static str] = &[Self::HEADER];

  #[must_use]
  fn from_reader(i_reader: R) -> Result<Self> {
    let mut lines = i_reader.lines();

    // Parse first line
    let mut s = lines.next().ok_or(Error::from(ErrorKind::ParseEofUnexpected)).chain_err(|| ErrorKind::ParseLine(1))??;
    // Pop separator
    let separator = s.pop().ok_or(Error::from(ErrorKind::ParseEolUnexpected)).chain_err(|| ErrorKind::ParseLine(1))?;
    // Check header
    if s != Self::HEADER {
      return Err(Error::from(ErrorKind::ParseTokenUnexpected(s, Self::HEADER_OPTIONS))).chain_err(|| ErrorKind::ParseLine(1));
    }

    // Create deserializer
    let mut d = Deserializer {
      input: lines,
      line_current: String::new(),
      line_index: 1,
      separator: lvm::Separator::try_from(separator)?,
      sequence_style: SequenceStyle::Following,
    };
    // Load the next line
    d.parse_newline()?;
    Ok(d)
  }

  #[must_use]
  fn deserialize<'de, T: serde::de::Deserialize<'de>>(&mut self) -> Result<T> {
    let r = T::deserialize(&mut *self);
    self.line_result(r)
  }

  #[must_use]
  fn line_result<T>(&self, r: Result<T>) -> Result<T> {
    r.chain_err(|| ErrorKind::ParseLine(self.line_index))
  }

  #[must_use]
  fn line_error<T>(&self, e: ErrorKind) -> Result<T> {
    Err(Error::from(e)).chain_err(|| ErrorKind::ParseLine(self.line_index))
  }

  #[must_use]
  fn peek_newline(&mut self) -> bool {
    self.line_current.is_empty()
  }

  #[must_use]
  fn parse_bool(&mut self) -> Result<bool> {
    match self.parse_token()?.as_ref() {
      Self::BOOL_NO => Ok(false),
      Self::BOOL_YES => Ok(true),
      t => self.line_error(ErrorKind::ParseTokenUnexpected(t.to_string(), Self::BOOL_OPTIONS))
    }
  }

  /*
  #[must_use]
  fn parse_char(&mut self) -> Result<char> {
    match self.parse_token()? {
      ref t if t.len() == 1 => { Ok(t.chars().next().unwrap()) },
      t => self.line_error(ErrorKind::ParseTokenUnexpected(t))
    }
  }
  */

  #[must_use]
  fn parse_integer<T: num::Integer>(&mut self) -> Result<T> where T: num::Num<FromStrRadixErr = std::num::ParseIntError> {
    Ok(T::from_str_radix(self.parse_token()?.as_ref(), 10)?)
  }

  #[must_use]
  fn parse_newline_or_eof(&mut self) -> Result<bool> {
    if self.line_current.is_empty() {
      match self.input.next() {
        Some(Ok(x)) => {
          self.line_current = x;
          self.line_index += 1;
          Ok(true)
        },
        Some(Err(e)) => self.line_result(Err(e.into())),
        None => Ok(false),
      }
    } else {
      self.line_error(ErrorKind::ParseEolExpected(self.line_current.to_string()))
    }
  }

  #[must_use]
  fn parse_newline(&mut self) -> Result<()> {
    if self.parse_newline_or_eof()? {
      Ok(())
    } else {
      self.line_error(ErrorKind::ParseEofUnexpected)
    }
  }

  #[must_use]
  fn parse_real<T: num::Float>(&mut self) -> Result<T> where T: num::Num<FromStrRadixErr = num::traits::ParseFloatError> {
    T::from_str_radix(self.parse_token()?.as_ref(), 10).map_err(|e|ErrorKind::ParseFloatError(e).into())
  }

  #[must_use]
  fn parse_separators(&mut self, i_count: usize) -> Result<()> {
    let separator: char = self.separator.into();
    for _ in 0..i_count {
      match self.line_current.chars().next() {
        Some(x) if x == separator => {
          self.line_current.remove(0);
          continue
        },
        Some(c) => {
          return self.line_error(ErrorKind::ParseSeparatorExpected(c, self.separator))
        },
        None => return self.line_error(ErrorKind::ParseEolUnexpected),
      }
    }

    Ok(())
  }

  #[must_use]
  fn parse_sequence(&mut self) -> Sequence<R> {
    Sequence::new(self.sequence_style, self)
  }

  #[must_use]
  fn parse_token(&mut self) -> Result<String> {
    let separator: char = self.separator.into();
    let mut old_line_current = String::new();
    std::mem::swap(&mut old_line_current, &mut self.line_current);
    match old_line_current.find(separator) {
      Some(index) => {
        self.line_current = old_line_current.split_off(index);
        Ok(old_line_current)
      },
      None if old_line_current.is_empty() => self.line_error(ErrorKind::ParseEolUnexpected),
      None => {
        Ok(old_line_current)
      }
    }
  }

  #[must_use]
  fn parse_tuple(&mut self, i_length: usize) -> Tuple<R> {
    Tuple::new(i_length, self)
  }

  fn set_sequence_style(&mut self, i_style: SequenceStyle) {
    self.sequence_style = i_style;
  }
}

struct Tuple<'a, R: std::io::BufRead + 'a> {
  de : &'a mut Deserializer<R>,
  length: usize,
  index: usize,
}

impl<'a, R: std::io::BufRead> Tuple<'a, R> {
  fn new(i_count: usize, i_de: &'a mut Deserializer<R>) -> Self {
    Tuple {
      de: i_de,
      index: 0,
      length: i_count
    }
  }
}

impl<'a, 'de: 'a, R: std::io::BufRead + 'a> serde::de::SeqAccess<'de> for Tuple<'a, R> {
  type Error = Error;

  fn next_element_seed<T: serde::de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
    if self.index >= self.length {
      Ok(None)
    } else {
      self.index += 1;
      seed.deserialize(&mut *self.de).map(Some)
    }
  }
}

#[derive(Clone,Copy,Debug)]
enum SequenceStyle {
  Following,
  FollowingSkipLast,
  Preceding,
}

struct Sequence<'a, R: std::io::BufRead + 'a> {
  de: &'a mut Deserializer<R>,
  first: bool,
  style: SequenceStyle,
}

impl<'a, R: std::io::BufRead> Sequence<'a, R> {
  fn new(i_style: SequenceStyle, i_de: &'a mut Deserializer<R>) -> Self {
    Sequence {
      de: i_de,
      first: true,
      style: i_style,
    }
  }
}

impl<'a, 'de: 'a, R: std::io::BufRead + 'a> serde::de::SeqAccess<'de> for Sequence<'a, R> {
  type Error = Error;

  fn next_element_seed<T: serde::de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
    match self.style {
      SequenceStyle::Following => {
        if !self.first { self.de.parse_separators(1)?; };
        if self.de.peek_newline() { return Ok(None) };
      },
      SequenceStyle::FollowingSkipLast => {
        if self.de.peek_newline() { return Ok(None) };
        if !self.first { self.de.parse_separators(1)? };
      },
      SequenceStyle::Preceding => {
        if self.de.peek_newline() { return Ok(None) };
        self.de.parse_separators(1)?;
      }
    }
    self.first = false;
    seed.deserialize(&mut *self.de).map(Some)
  }
}

impl<'de, R: std::io::BufRead> serde::de::MapAccess<'de> for Deserializer<R> {
  type Error = Error;

  fn next_key_seed<K: serde::de::DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>> {
    match self.parse_token()?.as_ref() {
      "***End_of_Header***" => Ok(None),
      t => seed.deserialize(t.into_deserializer()).map(Some)
    }
  }

  fn next_value_seed<V: serde::de::DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value> {
    self.parse_separators(1)?;
    let r = seed.deserialize(&mut *self)?;
    self.parse_newline()?;
    Ok(r)
  }
}

impl<'de, 'a, R: std::io::BufRead> serde::de::Deserializer<'de> for &'a mut Deserializer<R> {
  type Error = Error;

  fn deserialize_any<V: serde::de::Visitor<'de>>(self, _: V) -> Result<V::Value> {
    unimplemented!()
  }

  fn deserialize_bool<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_bool(self.parse_bool()?)
  }

  fn deserialize_byte_buf<V: serde::de::Visitor<'de>>(self, _: V) -> Result<V::Value> {
    unimplemented!()
  }

  fn deserialize_bytes<V: serde::de::Visitor<'de>>(self, _: V) -> Result<V::Value> {
    unimplemented!()
  }

  fn deserialize_char<V: serde::de::Visitor<'de>>(self, _v: V) -> Result<V::Value> {
    unimplemented!()
    //v.visit_char(self.parse_char()?)
  }

  fn deserialize_enum<V: serde::de::Visitor<'de>>(self, _name: &'static str, _variants: &'static [&'static str], v: V) -> Result<V::Value> {
    v.visit_enum(self.parse_token()?.into_deserializer())
  }

  fn deserialize_f32<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_f32(self.parse_real::<f32>()?)
  }

  fn deserialize_f64<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_f64(self.parse_real::<f64>()?)
  }

  fn deserialize_i8<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_i8(self.parse_integer::<i8>()?)
  }

  fn deserialize_i16<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_i16(self.parse_integer::<i16>()?)
  }

  fn deserialize_i32<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_i32(self.parse_integer::<i32>()?)
  }

  fn deserialize_i64<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_i64(self.parse_integer::<i64>()?)
  }

  fn deserialize_ignored_any<V: serde::de::Visitor<'de>>(self, _: V) -> Result<V::Value> {
    unimplemented!()
  }

  fn deserialize_seq<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_seq(self.parse_sequence())
  }

  fn deserialize_u8<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_u8(self.parse_integer::<u8>()?)
  }

  fn deserialize_u16<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_u16(self.parse_integer::<u16>()?)
  }

  fn deserialize_u32<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_u32(self.parse_integer::<u32>()?)
  }

  fn deserialize_u64<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_u64(self.parse_integer::<u64>()?)
  }

  fn deserialize_option<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    if self.peek_newline() {
      v.visit_none()
    } else {
      v.visit_some(self)
    }
  }

  fn deserialize_map<V: serde::de::Visitor<'de>>(self, _v: V) -> Result<V::Value> {
    unimplemented!()
  }

  fn deserialize_struct<V: serde::de::Visitor<'de>>(mut self, _name: &'static str, _fields: &'static [&'static str], v: V) -> Result<V::Value> {
    let r = v.visit_map(&mut self)?;
    self.parse_separators(1)?;
    Ok(r)
  }

  fn deserialize_identifier<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_string(self.parse_token()?)
  }

  fn deserialize_newtype_struct<V: serde::de::Visitor<'de>>(self, _name: &'static str, _v: V) -> Result<V::Value> {
    unimplemented!()
  }

  fn deserialize_str<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_string(self.parse_token()?)
  }

  fn deserialize_string<V: serde::de::Visitor<'de>>(self, v: V) -> Result<V::Value> {
    v.visit_string(self.parse_token()?)
  }

  fn deserialize_tuple<V: serde::de::Visitor<'de>>(self, len: usize, v: V) -> Result<V::Value> {
    v.visit_seq(self.parse_tuple(len))
  }

  fn deserialize_tuple_struct<V: serde::de::Visitor<'de>>(self, _name: &'static str, _len: usize, _v: V) -> Result<V::Value> {
    unimplemented!()
  }

  fn deserialize_unit<V: serde::de::Visitor<'de>>(self, _v: V) -> Result<V::Value> {
    unimplemented!()
  }

  fn deserialize_unit_struct<V: serde::de::Visitor<'de>>(self, _name: &'static str, _v: V) -> Result<V::Value> {
    unimplemented!()
  }
}

/// Deserializes LVM file data from the specified reader
pub fn from_reader<R: std::io::Read>(i_reader: R) -> Result<lvm::File> {
  let buf_reader = std::io::BufReader::new(i_reader);
  let mut deserializer = Deserializer::from_reader(buf_reader)?;

  let file_header: lvm::FileHeader = deserializer.deserialize()?;

  let file_measurements = {
    deserializer.parse_newline()?;
    deserializer.parse_separators(1)?;

    let mut measurements = vec![];
    loop {
      if !deserializer.parse_newline_or_eof()? {
        break;
      }
      deserializer.set_sequence_style(SequenceStyle::Following);
      let measurement_header: lvm::MeasurementHeader = deserializer.deserialize()?;
      deserializer.parse_separators(measurement_header.channels.0)?;
      deserializer.parse_newline()?;

      deserializer.set_sequence_style(SequenceStyle::FollowingSkipLast);
      let data_headings : Vec<String> = deserializer.deserialize()?;
      deserializer.parse_newline()?;

      deserializer.set_sequence_style(match file_header.x_columns {
        lvm::XColumns::No => SequenceStyle::Preceding,
        lvm::XColumns::One => SequenceStyle::FollowingSkipLast,
        _ => unimplemented!(),
      });
      let mut data_rows = vec![];
      loop {
        if deserializer.peek_newline() { break; }
        let data_row: lvm::DataRow = deserializer.deserialize()?;
        data_rows.push(data_row);
        if !deserializer.parse_newline_or_eof()? {
          break;
        }
      }

      measurements.push(lvm::Measurement {
        header: measurement_header,
        data_headings: data_headings,
        data: data_rows,
      });
    }
    measurements
  };

  let lvm_file = lvm::File {
    header: file_header,
    measurements: file_measurements,
  };

  Ok(lvm_file)
}
